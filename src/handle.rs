use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use bytes::BytesMut;
use http::Response;
use httparse::Request;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use crate::process::{Processor, write_response};

pub(crate) struct HandlerSupervisor {
	switch: Arc<AtomicU8>,
	processors: Arc<[Processor; 2]>,
}

impl HandlerSupervisor {
	#[allow(unused)]
	pub(crate) fn new() -> Self {
		let processors = Arc::new([Processor::new(), Processor::new()]);
		Self {
			switch: Arc::new(AtomicU8::new(0)),
			processors,
		}
	}

	pub(crate) fn root(root_path: PathBuf) -> Self {
		let processors = Arc::new([Processor::root(root_path.clone()), Processor::root(root_path)]);
		Self {
			switch: Arc::new(AtomicU8::new(0)),
			processors
		}
	}

	pub(crate) fn handler(&self) -> Handler {
		Handler {
			switch: self.switch.clone(),
			processors: self.processors.clone(),
			buffer: BytesMut::new(),
		}
	}

	async fn reset_unused(&self) -> u8 {
		let res = match self.switch.load(Ordering::SeqCst) {
			0 => 1,
			1 => 0,
			_ => unimplemented!(),
		};
		match res {
			0 => self.processors[0].reset().await,
			1 => self.processors[1].reset().await,
			_ => unimplemented!(),
		}
		res
	}

	#[allow(unused)]
	pub(crate) async fn reset_and_switch(&self) {
		let reseted_processor = self.reset_unused().await;
		self.switch.store(reseted_processor, Ordering::SeqCst);
	}
}

pub(crate) struct Handler {
	switch: Arc<AtomicU8>,
	processors: Arc<[Processor; 2]>,
	buffer: BytesMut,
}

impl Handler {
	pub(crate) async fn handle<T: AsyncRead + AsyncWrite + Unpin + Sized>(&mut self, mut conn: T) -> std::io::Result<()> {
		let path = loop {
			conn.read_buf(&mut self.buffer).await?;

			let mut inner_headers = [httparse::EMPTY_HEADER; 64];
			let mut request = Request::new(&mut inner_headers);

			match request.parse(&self.buffer) {
				Err(_) => {
					write_invalid_request(conn).await?;
					return Ok(());
				}
				Ok(httparse::Status::Partial) => {
					if request.path.is_some() {
						break Cow::Owned(request.path.unwrap().to_string());
					} else {
						continue;
					}
				}
				Ok(httparse::Status::Complete(_)) => {
					break Cow::Owned(request.path.unwrap().to_string());
				}
			};
		};

		match self.switch.load(Ordering::Relaxed) {
			0 => self.processors[0].0.read().await.process_request(conn, &path, &mut self.buffer).await,
			1 => self.processors[1].0.read().await.process_request(conn, &path, &mut self.buffer).await,
			_ => panic!("switch contains unexpected value"),
		}
	}
}

async fn write_invalid_request<T: AsyncRead + AsyncWrite + Unpin + Sized>(mut conn: T) -> std::io::Result<()> {
	let invalid_response = Response::builder()
		.status(400)
		.header("content-length", "0")
		.body("")
		.unwrap();

	write_response(&mut conn, &invalid_response).await?;
	Ok(())
}