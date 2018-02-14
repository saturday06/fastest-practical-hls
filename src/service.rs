use futures;
use hyper;
use futures::future::Future;
use hyper::{Get, StatusCode};
use hyper::header::{ContentLength, ContentType, Location};
use hyper::server::{Request, Response, Service};
use hls::Hls;
use std::sync::{Arc, RwLock};
use std::path::PathBuf;
use std::fs::{canonicalize, File};
use std::error::Error;
use std::io::copy;
use lazybytes::LazyBytesStream;
use futures::Stream;
use futures::stream::once;

type Body = Box<Stream<Item = hyper::Chunk, Error = hyper::Error>>;

pub struct HlsService {
    hls: Arc<RwLock<Hls>>,
}

impl HlsService {
    pub fn new(hls: Arc<RwLock<Hls>>) -> HlsService {
        HlsService { hls }
    }
}

impl Service for HlsService {
    type Request = Request;
    type Response = Response<Body>;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        const SEGMENT_PREFIX: &str = "/segment";
        Box::new(futures::future::ok(match (req.method(), req.path()) {
            (&Get, path) if path.starts_with(SEGMENT_PREFIX) => {
                match path.replace(SEGMENT_PREFIX, "")
                    .replace(".ts", "")
                    .parse::<u64>()
                {
                    Ok(segment_index) => match {
                        let lock = self.hls
                            .as_ref()
                            .read()
                            .expect("Failed to lock internal resource for reading hls segment");
                        let hls = &*lock;
                        hls.read_segment(segment_index)
                    } {
                        Some(segment) => {
                            let body: Body = Box::new(LazyBytesStream::new(segment));
                            Response::new().with_body(body)
                        }
                        _ => Response::new().with_status(StatusCode::NotFound),
                    },
                    Err(err) => {
                        let body = format!("Invalid segment index: {}", err.description());
                        Response::new()
                            .with_header(ContentLength(body.len() as u64))
                            .with_status(StatusCode::BadRequest)
                    }
                }
            }
            (&Get, "/index.m3u8") => {
                let playlist = {
                    let lock = self.hls
                        .as_ref()
                        .read()
                        .expect("Failed to lock internal resource for reading hls playlist");
                    let hls = &*lock;
                    hls.generate_playlist()
                };
                let content_type_str = "application/vnd.apple.mpegurl";
                let content_type = content_type_str
                    .parse()
                    .expect(&format!("Failed to parse {} as mime", content_type_str));
                let playlist_len = playlist.len();
                let body: Body = Box::new(once(Ok(hyper::Chunk::from(playlist))));
                Response::new()
                    .with_header(ContentLength(playlist_len as u64))
                    .with_header(ContentType(content_type))
                    .with_body(body)
            }
            (&Get, "/") => {
                Response::new()
                    .with_header(Location::new("/index.html?src=index.m3u8&enableStreaming=true&autoRecoverError=true&enableWorker=true&dumpfMP4=false&levelCapping=-1&defaultAudioCodec=undefined&widevineLicenseURL="))
                    .with_status(StatusCode::SeeOther)
            }
            (&Get, file_path_str) => {
                let file_path = PathBuf::from(file_path_str);
                let mut path = canonicalize(PathBuf::from(file!())).expect("file!!!");
                assert!(path.pop());
                assert!(path.pop());
                path.push("www");
                path.push(file_path.file_name().expect("no file name!"));
                // println!("static: {:?}", path);
                match File::open(path) {
                    Ok(mut file) => {
                        let mut buf: Vec<u8> = Vec::new();
                        match copy(&mut file, &mut buf) {
                            Ok(_) => {
                                let buf_len = buf.len();
                                let body: Body = Box::new(once(Ok(hyper::Chunk::from(buf))));
                                Response::new()
                                    .with_header(ContentLength(buf_len as u64))
                                    .with_body(body)
                            },
                            Err(_) => Response::new().with_status(StatusCode::NotFound),
                        }
                    }
                    Err(_) => Response::new().with_status(StatusCode::NotFound),
                }
            }
            _ => Response::new().with_status(StatusCode::NotFound),
        }))
    }
}
