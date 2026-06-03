use std::path::Path;

use tokio::{fs, io, net::UnixListener};

pub async fn bind_unix_socket<P: AsRef<Path>>(path: P) -> Result<UnixListener, io::Error> {
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let _ = fs::remove_file(path).await;

    UnixListener::bind(path)
}
