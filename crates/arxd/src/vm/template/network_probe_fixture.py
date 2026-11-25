# Appended to the sealed guest program by its Rust fixture test.
import errno
import socket
import subprocess
import time
from unittest.mock import MagicMock, patch

NONCE = bytes(16)
for code in (
    errno.ENETUNREACH, errno.EHOSTUNREACH, errno.ENODEV,
    errno.EADDRNOTAVAIL, errno.ECONNREFUSED, errno.ETIMEDOUT,
):
    for probe in (probe_tcp, probe_dns):
        for addr in ('192.0.2.1', '2001:db8::1'):
            sock = MagicMock()
            sock.connect.side_effect = OSError(code, 'fixture')
            with patch.object(socket, 'socket') as constructor:
                constructor.return_value.__enter__.return_value = sock
                assert probe((addr, 53), '10.0.2.15') == 'E'
                sock.send.assert_not_called()

TCP = [('192.0.2.1', 53)] * 6
DNS = TCP[0]
nic = MagicMock()
nic.name = 'eth0'
nic.__truediv__.return_value.resolve.return_value.name = 'virtio_net'
nic.__truediv__.return_value.read_text.return_value = 'up'
for devices, lease, installs, expected in [
    ([], None, False, '0000'),
    ([nic], None, False, '1001'),
    ([nic], '10.0.2.15', False, '1101'),
    ([nic], '10.0.2.15', True, '1111'),
]:
    route = False

    def install(args, **kwargs):
        global route
        assert args == ['ip', '-4', 'route', 'add', 'default', 'via', '10.0.2.2', 'dev', 'eth0']
        route = installs

    nic_ipv4 = lambda name: lease
    has_slirp_default_route = lambda name: route
    ipv6_is_disabled = lambda name: True
    probe_tcp, probe_dns = MagicMock(return_value='R'), MagicMock(return_value='R')
    with patch.object(time, 'sleep'), patch.object(subprocess, 'run', side_effect=install) as add:
        setup, outcomes = run_probes(devices)
        assert setup == expected
        if expected == '1111':
            assert outcomes == 'R' * 8
            add.assert_called_once()
        else:
            assert outcomes == 'E' * 8
            probe_tcp.assert_not_called()
            probe_dns.assert_not_called()
        if not lease:
            add.assert_not_called()
