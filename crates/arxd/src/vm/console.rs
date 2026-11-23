//! Typed read-only capture over a runtime-owned QMP channel. The runtime
//! supplies an anchored, generation-owned artifact and reserves its disk
//! writes before issuing a capture request.
use super::qmp::{ChannelError, Qmp};
use axum::body::Bytes;
use std::{io::Read, path::Path};
use tokio::io::{AsyncRead, AsyncWrite};

pub const MAX_PNG: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    Stopped,
    Replaced,
    Fault,
}

pub struct Frame {
    bytes: Bytes,
    width: u32,
    height: u32,
}

impl Frame {
    pub fn from_png(bytes: Bytes) -> Result<Self, CaptureError> {
        let invalid = CaptureError::Fault;
        if bytes.len() > MAX_PNG || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(invalid);
        }
        let word = |bytes: &[u8]| {
            bytes
                .get(..4)
                .map(|field| u32::from_be_bytes(field.try_into().expect("four bytes")))
        };
        let mut tail = bytes.get(8..).ok_or(invalid)?;
        let mut dimensions = None;
        let mut data = false;
        while !tail.is_empty() {
            let len = word(tail).ok_or(invalid)? as usize;
            let chunk = tail
                .get(..len.checked_add(12).ok_or(invalid)?)
                .ok_or(invalid)?;
            tail = &tail[chunk.len()..];
            match &chunk[4..8] {
                b"IHDR" if dimensions.is_none() && len == 13 => {
                    let width = word(&chunk[8..]).ok_or(invalid)?;
                    let height = word(&chunk[12..]).ok_or(invalid)?;
                    if !(1..=8192).contains(&width) || !(1..=8192).contains(&height) {
                        return Err(invalid);
                    }
                    dimensions = Some((width, height));
                }
                _ if dimensions.is_none() => return Err(invalid),
                b"IHDR" => return Err(invalid),
                b"IDAT" => data |= len != 0,
                b"IEND" if len == 0 && data && tail.is_empty() => {
                    let (width, height) = dimensions.ok_or(invalid)?;
                    return Ok(Self {
                        bytes,
                        width,
                        height,
                    });
                }
                b"IEND" => return Err(invalid),
                _ => {}
            }
        }
        Err(invalid)
    }

    pub fn image(&self) -> (u32, u32, Bytes) {
        (self.width, self.height, self.bytes.clone())
    }
}

/// A private, exclusive capture leaf in a pinned runtime directory. The
/// caller holds directory and volume custody, rejects substitutions and
/// reparse points at open, and reconciles ambiguous leaves after QMP faults.
pub trait CaptureArtifact: Send + 'static {
    fn path(&self) -> &Path;
    fn open_verified(&self) -> std::io::Result<Box<dyn Read>>;
    fn remove_verified(self) -> std::io::Result<()>;
}

/// Run on the QMP owner. A viewer disconnect must allow this request to
/// finish, including its bounded QMP deadline and correlated file cleanup.
pub async fn capture<R, W>(
    qmp: &mut Qmp<R, W>,
    artifact: impl CaptureArtifact,
) -> Result<Frame, CaptureError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let filename = artifact.path().to_str().ok_or(CaptureError::Fault)?;
    if !artifact.path().is_absolute() {
        return Err(CaptureError::Fault);
    }
    let result = qmp.screendump(filename).await;
    if !matches!(result, Ok(()) | Err(ChannelError::Remote)) {
        // QMP cannot prove the writer has stopped. Leave the leaf to the
        // runtime's reconciliation under its original custody and cap.
        return Err(CaptureError::Fault);
    }
    tokio::task::spawn_blocking(move || {
        let frame = (|| {
            result.map_err(|_| CaptureError::Fault)?;
            let mut data = Vec::new();
            artifact
                .open_verified()
                .map_err(|_| CaptureError::Fault)?
                .take((MAX_PNG + 1) as u64)
                .read_to_end(&mut data)
                .map_err(|_| CaptureError::Fault)?;
            Frame::from_png(data.into())
        })();
        artifact
            .remove_verified()
            .map_err(|_| CaptureError::Fault)?;
        frame
    })
    .await
    .map_err(|_| CaptureError::Fault)?
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{dashboard::tests::private_dir, service::Shutdown, vm::qemu_args::QEMU_VERSION};
    use serde_json::{Value, json};
    use std::path::PathBuf;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    pub const PNG: &str = "89504E470D0A1A0A0000000D4948445200000001000000010804000000B51C0C020000000B4944415478DA63FCFF1F0003030200EFA345990000000049454E44AE426082";

    #[test]
    fn zero_png_side_is_rejected() {
        let mut png = hex::decode(PNG).unwrap();
        png[16..20].fill(0);
        assert!(matches!(
            Frame::from_png(png.into()),
            Err(CaptureError::Fault)
        ));
    }

    struct FixtureFile(PathBuf);

    impl CaptureArtifact for FixtureFile {
        fn path(&self) -> &Path {
            &self.0
        }

        fn open_verified(&self) -> std::io::Result<Box<dyn Read>> {
            let metadata = self.0.symlink_metadata()?;
            assert!(metadata.is_file());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            }
            #[cfg(windows)]
            crate::local::owned(&self.0).unwrap();
            crate::security::private(&self.0).unwrap();
            Ok(Box::new(std::fs::File::open(&self.0)?))
        }

        fn remove_verified(self) -> std::io::Result<()> {
            std::fs::remove_file(self.0)
        }
    }

    fn write_private(path: &Path, data: &[u8]) {
        use std::{
            fs::{self, OpenOptions},
            io::Write,
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        #[cfg(windows)]
        crate::dashboard::tests::set_owner_private(path);
        file.write_all(data).unwrap();
        assert!(fs::symlink_metadata(path).unwrap().is_file());
    }

    #[tokio::test]
    async fn typed_screendump_reads_bounded_private_png_and_removes_each_leaf() {
        let scratch = crate::service::application::tests::private_temp();
        let root = scratch.path().join("capture");
        private_dir(&root);
        let (a, b) = tokio::io::duplex(4096);
        let (r, w) = tokio::io::split(a);
        let (peer_r, mut peer_w) = tokio::io::split(b);
        let root_copy = root.clone();
        let peer = tokio::spawn(async move {
            let mut commands = BufReader::new(peer_r).lines();
            let version: Vec<u16> = QEMU_VERSION
                .split('.')
                .map(|s| s.parse().unwrap())
                .collect();
            let greeting = json!({"QMP":{"version":{"qemu":{
                "major":version[0],"minor":version[1],"micro":version[2]},"package":""},"capabilities":[]}});
            peer_w
                .write_all(format!("{greeting}\n").as_bytes())
                .await
                .unwrap();
            let command: Value =
                serde_json::from_str(&commands.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(command["execute"], "qmp_capabilities");
            peer_w
                .write_all(b"{\"return\":{},\"id\":1}\n")
                .await
                .unwrap();
            for (index, data) in [
                hex::decode(PNG).unwrap(),
                b"bad".to_vec(),
                vec![0; MAX_PNG + 1],
            ]
            .into_iter()
            .enumerate()
            {
                let command: Value =
                    serde_json::from_str(&commands.next_line().await.unwrap().unwrap()).unwrap();
                let leaf = root_copy.join(format!("dump-{index}.png"));
                assert_eq!(
                    command,
                    json!({"execute":"screendump","id":index + 2,"arguments":{
                    "filename":leaf.to_str().unwrap(),"format":"png","device":"arxdisplay0","head":0}})
                );
                write_private(&leaf, &data);
                peer_w
                    .write_all(format!("{{\"return\":{{}},\"id\":{}}}\n", index + 2).as_bytes())
                    .await
                    .unwrap();
            }
        });
        let mut qmp = Qmp::handshake(r, w, &Shutdown::default()).await.unwrap();
        for index in 0..3 {
            let leaf = root.join(format!("dump-{index}.png"));
            let frame = capture(&mut qmp, FixtureFile(leaf.clone())).await;
            assert!(!leaf.exists());
            if index == 0 {
                let (width, height, image) = frame.unwrap().image();
                assert_eq!((width, height), (1, 1));
                assert_eq!(image.as_ref(), hex::decode(PNG).unwrap());
            } else {
                assert!(matches!(frame, Err(CaptureError::Fault)));
            }
        }
        peer.await.unwrap();
    }
}
