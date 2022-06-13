use tokio::net::TcpListener;
use clap::Parser;

use crate::handle::HandlerSupervisor;
use crate::limits::set_soft_limit_to_hard;
use crate::options::{Args, Host};

mod handle;
mod process;
mod limits;
mod options;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args: Args = Args::parse();

    set_soft_limit_to_hard();

    let handler_supervisor = HandlerSupervisor::root(args.path);

    match args.host {
        Host::Tcp { addr } => {
            let listener = TcpListener::bind(addr).await?;

            loop {
                let mut handler = handler_supervisor.handler();

                let (new_conn, _) = listener.accept().await?;

                tokio::spawn(async move {
                    handler.handle(new_conn).await
                });
            }
        }
        Host::Unix { path } => {
            cfg_if::cfg_if! {
                if #[cfg(unix)] {
                    use tokio::net::UnixListener;

                    let listener = UnixListener::bind(path)?;

                    loop {
                        let mut handler = handler_supervisor.handler();

                        let (new_conn, _) = listener.accept().await?;

                        tokio::spawn(async move {
                            handler.handle(new_conn).await
                        });
                    }
                } else {
                    eprintln!("unix listener not allowed on non unix system")
                }
            }
        }
    }
}
