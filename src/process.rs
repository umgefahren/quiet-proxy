use std::borrow::{Borrow, Cow};
use std::io::ErrorKind;
use std::ops::Not;
use std::path::{Path, PathBuf};
use bytes::{BufMut, BytesMut};
use chashmap::CHashMap;
use http::{Response, StatusCode};
use itoa::Buffer;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::RwLock;

const DEFAULT_404_PAGE: &str = include_str!("../assets/404.html");

pub(crate) struct Processor(pub(crate) RwLock<ProcessorInner>);

impl Processor {
	pub(crate) fn new() -> Self {
		Self(RwLock::new(ProcessorInner::new()))
	}

	pub(crate) fn root(root_path: PathBuf) -> Self {
		Self(RwLock::new(ProcessorInner::root(root_path)))
	}

	pub(crate) async fn reset(&self) {
		self.0.write().await.reset()
	}
}

pub(crate) struct ProcessorInner {
	root_path: PathBuf,
	not_found_page: Cow<'static, str>,
	resolved_paths: CHashMap<String, PathBuf>,
}

impl ProcessorInner {
	fn new() -> Self {
		Self {
			root_path: PathBuf::from("."),
			not_found_page: Cow::Borrowed(DEFAULT_404_PAGE),
			resolved_paths: CHashMap::new(),
		}
	}

	fn root(root_path: PathBuf) -> Self {
		Self {
			root_path,
			not_found_page: Cow::Borrowed(DEFAULT_404_PAGE),
			resolved_paths: CHashMap::new(),
		}
	}

	fn reset(&mut self) {
		self.root_path = PathBuf::from(".");
		self.not_found_page = Cow::Borrowed(DEFAULT_404_PAGE);
		self.resolved_paths = CHashMap::new();
	}

	pub(crate) async fn process_request<'a, T: AsyncWrite + AsyncRead + Unpin + Sized>(&'a self, conn: T, path: &'a str, buffer: &mut BytesMut) -> std::io::Result<()> {
		let resolve_result = self.resolve_path_to_file(path).await;

		match resolve_result {
			Some((file, path_buf)) => {
				if self.resolved_paths.contains_key(path).not() {
					self.resolved_paths.borrow().insert(path.to_string(), path_buf.clone());
				}

				self.respond_with_file(conn, file, path_buf, buffer).await
			},
			None => self.respond_not_found(conn).await
		}
	}

	async fn respond_with_file<T: AsyncWrite + AsyncRead + Unpin + Sized>(&self, mut conn: T, file: File, path: PathBuf, _buffer: &mut BytesMut) -> std::io::Result<()> {
		let file_metadata = file.metadata().await?;
		let file_size = file_metadata.len();
		let mut itoa_buffer = Buffer::new();
		let file_size_str = itoa_buffer.format(file_size);
		let content_type = match path.extension().map(|x| x.to_str()) {
			Some(Some("html")) => Some("text/html"),
			Some(Some("js")) => Some("application/javascript"),
			Some(Some("pdf")) => Some("application/pdf"),
			Some(Some("json")) => Some("application/json"),
			Some(Some("zip")) => Some("application/zip"),
			Some(Some("jpg")) | Some(Some("jpeg")) => Some("image/jpeg"),
			Some(Some("png")) => Some("image/png"),
			Some(Some("gif")) => Some("image/gif"),
			Some(Some("csv")) => Some("text/csv"),
			Some(Some("css")) => Some("text/css"),
			Some(Some("php")) => Some("text/php"),
			None => None,
			_ => None,
		};

		let mut response_builder = Response::builder()
			.status(StatusCode::OK);

		if content_type.is_some() {
			response_builder = response_builder.header("Content-Type", content_type.unwrap());
		}

		let response = response_builder
			.header("Content-Length", file_size_str)
			.body(())
			.unwrap();

		let mut response_buffer = BytesMut::new();

		gen_response_wo_body(&response, &mut response_buffer);

		let mut chain = AsyncReadExt::chain(response_buffer.as_ref(), file);

		tokio::io::copy(&mut chain, &mut conn).await?;

		Ok(())
	}

	async fn respond_not_found<T: AsyncWrite + AsyncRead + Unpin + Sized>(&self, mut conn: T) -> std::io::Result<()> {
		let not_found_size = self.not_found_page.len();
		let mut itoa_buffer = itoa::Buffer::new();
		let not_found_size_str = itoa_buffer.format(not_found_size);
		let response = Response::builder()
			.status(StatusCode::NOT_FOUND)
			.header("Content-Type", "text/html")
			.header("Content-Length", not_found_size_str)
			.body(self.not_found_page.as_ref())
			.unwrap();

		write_response(&mut conn, &response).await
	}

	async fn resolve_path_to_file(&self, path: &str) -> Option<(File, PathBuf)> {
		if let Some(d) = self.resolved_paths.get(path) {
			let path_buf = d.clone();
			if let Ok(file) = File::open(&path_buf).await {
				return Some((file, path_buf));
			}
		}

		let cleaned_path = match path.strip_prefix('/').unwrap() {
			"" => "index.html",
			d => d,
		};
		let joined_path = self.root_path.join(Path::new(cleaned_path));
		let open_options_result = OpenOptions::new()
			.read(true)
			.open(&joined_path)
			.await;

		match open_options_result {
			Ok(d) => {
				if d.metadata().await.unwrap().is_dir() {
					return with_index_extension(&joined_path).await;
				}
				Some((d, joined_path))
			},
			Err(e) if e.kind() == ErrorKind::NotFound => with_index_extension(&joined_path).await,
			Err(_) => None,
		}
	}
}

#[inline]
async fn with_index_extension(joined: &Path) -> Option<(File, PathBuf)>  {
	let index_path = joined.join(Path::new("./index.html"));
	let open_options_result = OpenOptions::new()
		.read(true)
		.open(&index_path)
		.await;
	open_options_result.ok().map(|x| (x, index_path))
}

macro_rules! status_line_lit {
    ($c:literal $r:literal) => {
		Cow::Borrowed(concat!("HTTP/1.1 ", $c, " ", $r, "\r\n"))
	};
}

fn gen_status_line_str<B>(response: &Response<B>) -> Cow<'static, str> {
	let status = response.status();
	match status.as_u16() {
		200 => status_line_lit!(200 "OK"),
		404 => status_line_lit!(404 "Not Found"),
		_ => {
			let status_num = status.as_u16();
			let mut itoa_buffer = Buffer::new();
			let status_num_str = itoa_buffer.format(status_num);
			let resp_str = format!("HTTP/1.1 {} {}\r\n", status_num_str, status.canonical_reason().unwrap_or(""));
			Cow::Owned(resp_str)
		},
	}
}

fn gen_response_wo_body<B>(response: &Response<B>, resp_buf: &mut BytesMut) {
	let status_line = gen_status_line_str(response);
	resp_buf.put_slice(status_line.as_bytes());
	response
		.headers()
		.iter()
		.for_each(|(name, value)| {
			resp_buf.put_slice(name.as_ref());
			resp_buf.put_slice(": ".as_ref());
			resp_buf.put_slice(value.as_bytes());
			resp_buf.put_slice("\r\n".as_ref());
		});
	resp_buf.put_slice("\r\n".as_ref());
}

pub async fn write_response<T: AsyncWrite + AsyncRead + Unpin + Sized, B: AsRef<[u8]>>(conn: &mut T, response: &Response<B>) -> std::io::Result<()> {
	let mut response_buffer = BytesMut::new();
	gen_response_wo_body(response, &mut response_buffer);
	response_buffer.put_slice(response.body().as_ref());

	conn.write_all_buf(&mut response_buffer).await?;
	Ok(())
}