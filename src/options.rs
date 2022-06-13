use std::net::SocketAddr;
use std::path::PathBuf;
use clap::{Parser, Subcommand};

/// Quiet proxy
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub(crate) struct Args {
	/// static hosting path
	///
	/// Path of the directory which will be hosted statically
	#[clap(short, long, default_value = ".", env = "STATIC_PATH")]
	pub(crate) path: PathBuf,
	/// kind of hosting
	#[clap(subcommand)]
	pub(crate) host: Host,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Host {
	Tcp {
		#[clap(env)]
		addr: SocketAddr,
	},
	Unix {
		#[clap(env = "SOCKET_PATH")]
		path: PathBuf
	},
}