//! Run-local, non-admin witnesses for the reviewed restricted slirp plan.

use std::{
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    task::JoinSet,
    time::{Instant, timeout},
};

use crate::{service::Shutdown, vm::guest::network_probe::dns_query};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Addresses {
    pub ipv4: Ipv4Addr,
    pub ipv6: Ipv6Addr,
    pub dns: Ipv4Addr,
}

impl Addresses {
    pub(super) fn valid(&self, resolvers: &[std::net::IpAddr]) -> bool {
        let valid_v4 = |addr: Ipv4Addr| {
            !addr.is_unspecified()
                && !addr.is_loopback()
                && !addr.is_multicast()
                && addr != Ipv4Addr::BROADCAST
                && addr.octets()[..3] != [10, 0, 2]
        };
        valid_v4(self.ipv4)
            && self.ipv6.to_ipv4_mapped().is_none()
            && !self.ipv6.is_unspecified()
            && !self.ipv6.is_loopback()
            && !self.ipv6.is_multicast()
            && !self.ipv6.is_unicast_link_local()
            && valid_v4(self.dns)
            // An unobserved fallback resolver would make the alias unsafe to
            // attest. The DNS listener must bind this exact host address.
            && resolvers == [std::net::IpAddr::V4(self.dns)]
    }
}

/// The three TCP and UDP roles are IPv4, IPv6 and the slirp host alias.
/// Both direct and slirp-alias DNS attempts terminate at one DNS witness.
pub(in crate::vm) struct Targets {
    pub tcp: [SocketAddr; 3],
    pub udp: [SocketAddr; 3],
    pub dns: SocketAddr,
}

pub(super) struct Witnesses {
    group: JoinSet<io::Result<()>>,
    stop: Shutdown,
    armed: Arc<AtomicBool>,
    receipts: Receipts,
}

type Receipts = Arc<Mutex<[Option<Instant>; 7]>>;

impl Witnesses {
    pub(super) async fn prepare(
        addresses: &Addresses,
        nonce: [u8; 16],
    ) -> io::Result<(Self, Targets)> {
        let ips = [
            addresses.ipv4.into(),
            addresses.ipv6.into(),
            Ipv4Addr::LOCALHOST.into(),
        ];
        Self::bind(ips, (addresses.dns, 53).into(), nonce).await
    }

    async fn bind(
        ips: [std::net::IpAddr; 3],
        dns_address: SocketAddr,
        nonce: [u8; 16],
    ) -> io::Result<(Self, Targets)> {
        let mut sockets = Vec::with_capacity(3);
        for ip in ips {
            sockets.push((
                TcpListener::bind((ip, 0)).await?,
                UdpSocket::bind((ip, 0)).await?,
            ));
        }
        let dns = UdpSocket::bind(dns_address).await?;
        let dns_address = dns.local_addr()?;
        let mut held = Self {
            group: JoinSet::new(),
            stop: Shutdown::default(),
            armed: Arc::new(AtomicBool::new(false)),
            receipts: Arc::new(Mutex::new([None; 7])),
        };
        let mut tcp = Vec::with_capacity(3);
        let mut udp = Vec::with_capacity(3);
        let mut control_nonce = [0; 16];
        getrandom::fill(&mut control_nonce).map_err(|_| io::Error::other("witness nonce"))?;
        if control_nonce == nonce {
            return Err(io::Error::other("witness nonce reused"));
        }
        for (role, (stream, datagrams)) in sockets.into_iter().enumerate() {
            tcp.push(stream.local_addr()?);
            udp.push(datagrams.local_addr()?);
            let (stop, armed, receipts) = held.state();
            held.group.spawn(async move {
                for _ in 0..16 {
                    let (mut peer, _) = tokio::select! {
                        biased;
                        accept = stream.accept() => accept?,
                        _ = stop.cancelled() => return Ok(()),
                    };
                    if armed.load(Ordering::Acquire) {
                        record(&receipts, role)?;
                        return Ok(());
                    }
                    peer.write_all(&control_nonce).await?;
                }
                Err(io::Error::other("witness connection budget exceeded"))
            });
            held.watch_udp(datagrams, role + 3, nonce, control_nonce, false);
        }
        held.watch_udp(dns, 6, nonce, control_nonce, true);
        let controls = async {
            for &addr in &tcp {
                timeout(CONTROL_TIMEOUT, async {
                    let mut stream = TcpStream::connect(addr).await?;
                    let mut reply = [0; 16];
                    stream.read_exact(&mut reply).await?;
                    exact(reply == control_nonce)
                })
                .await??;
            }
            for &addr in &udp {
                udp_control(addr, &control_nonce, &control_nonce).await?;
            }
            let query = dns_query(&control_nonce);
            udp_control(dns_address, &query, &dns_answer(&query)).await
        }
        .await;
        if let Err(error) = controls {
            let _ = held.settle().await;
            return Err(error);
        }
        // The addresses were tested via Windows. Only the guest uses the
        // slirp IPv4 alias instead of the actual loopback address.
        tcp[2].set_ip(Ipv4Addr::new(10, 0, 2, 2).into());
        udp[2].set_ip(Ipv4Addr::new(10, 0, 2, 2).into());
        held.armed.store(true, Ordering::Release);
        Ok((
            held,
            Targets {
                tcp: tcp.try_into().expect("three TCP roles"),
                udp: udp.try_into().expect("three UDP roles"),
                dns: dns_address,
            },
        ))
    }

    fn state(&self) -> (Shutdown, Arc<AtomicBool>, Receipts) {
        (self.stop.clone(), self.armed.clone(), self.receipts.clone())
    }

    fn watch_udp(
        &mut self,
        socket: UdpSocket,
        role: usize,
        nonce: [u8; 16],
        control: [u8; 16],
        dns: bool,
    ) {
        let (stop, armed, receipts) = self.state();
        self.group.spawn(async move {
            let mut buffer = [0; 513];
            let query = dns.then(|| dns_query(&nonce));
            let control_query = dns.then(|| dns_query(&control));
            // A bounded amount of unrecognized traffic keeps setup/settlement
            // conservative under load. Payloads never leave this function.
            for _ in 0..64 {
                let (size, source) = tokio::select! {
                    biased;
                    datagram = socket.recv_from(&mut buffer) => datagram?,
                    _ = stop.cancelled() => return Ok(()),
                };
                let received = &buffer[..size];
                if armed.load(Ordering::Acquire) {
                    if received == query.as_deref().unwrap_or(&nonce) {
                        record(&receipts, role)?;
                        if dns {
                            socket.send_to(&dns_answer(received), source).await?;
                        }
                        return Ok(());
                    }
                } else if received == control_query.as_deref().unwrap_or(&control) {
                    if dns {
                        socket.send_to(&dns_answer(received), source).await?;
                    } else {
                        socket.send_to(&control, source).await?;
                    }
                }
            }
            Err(io::Error::other("witness datagram budget exceeded"))
        });
    }

    pub(super) async fn settle(mut self) -> io::Result<u16> {
        self.stop.cancel();
        let drained = timeout(CLOSE_TIMEOUT, async {
            while let Some(task) = self.group.join_next().await {
                task??;
            }
            io::Result::Ok(())
        })
        .await;
        if !matches!(drained, Ok(Ok(()))) {
            self.group.shutdown().await;
        }
        drained??;
        let receipts = self
            .receipts
            .lock()
            .map_err(|_| io::Error::other("witness receipts unavailable"))?;
        Ok(receipts
            .iter()
            .enumerate()
            .filter_map(|(role, at)| at.map(|_| 1 << role))
            .sum())
    }
}

fn record(receipts: &Receipts, role: usize) -> io::Result<()> {
    let mut receipts = receipts
        .lock()
        .map_err(|_| io::Error::other("witness receipts unavailable"))?;
    receipts[role] = Some(Instant::now());
    Ok(())
}

async fn udp_control(addr: SocketAddr, request: &[u8], expected: &[u8]) -> io::Result<()> {
    timeout(CONTROL_TIMEOUT, async {
        let local: SocketAddr = if addr.is_ipv6() {
            (Ipv6Addr::UNSPECIFIED, 0).into()
        } else {
            (Ipv4Addr::UNSPECIFIED, 0).into()
        };
        let socket = UdpSocket::bind(local).await?;
        socket.connect(addr).await?;
        socket.send(request).await?;
        let mut response = [0; 513];
        let size = socket.recv(&mut response).await?;
        exact(&response[..size] == expected)
    })
    .await?
}

fn dns_answer(query: &[u8]) -> Vec<u8> {
    let mut answer = query.to_vec();
    answer[2] = 0x81;
    answer[3] = 0x83;
    answer
}

fn exact(ok: bool) -> io::Result<()> {
    ok.then_some(())
        .ok_or_else(|| io::Error::other("wrong witness control"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn host_witnesses_distinguish_silence_nonce_and_connection() {
        let ip = Ipv4Addr::LOCALHOST.into();
        let hosts = [ip, Ipv6Addr::LOCALHOST.into(), ip];
        let nonce = [8; 16];
        let dns = (ip, 0).into();
        let (held, _) = Witnesses::bind(hosts, dns, nonce).await.unwrap();
        assert_eq!(held.settle().await.unwrap(), 0);

        let (held, targets) = Witnesses::bind(hosts, dns, nonce).await.unwrap();
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        socket.send_to(&[7; 16], targets.udp[0]).await.unwrap();
        socket.send_to(&nonce, targets.udp[0]).await.unwrap();
        let _connection = TcpStream::connect(targets.tcp[1]).await.unwrap();
        socket
            .send_to(&dns_query(&nonce), targets.dns)
            .await
            .unwrap();
        let mut reply = [0; 512];
        let _ = timeout(CONTROL_TIMEOUT, socket.recv(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(held.settle().await.unwrap(), 0b1001010);
    }
}
