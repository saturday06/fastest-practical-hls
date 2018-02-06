use futures;
use hyper;
use futures::future::Future;
use hyper::{Body, Chunk, Get, StatusCode};
use hyper::header::{ContentLength, ContentType, Location};
use hyper::server::{Request, Response, Service};
use hls::Hls;
use std::sync::{Arc, RwLock};
use std::error;
use std::io;
use std::path::PathBuf;
use std::fs::{canonicalize, File};
use std::io::copy;
use std::thread;
use std::time;
use lazybytes::LazyBytesStream;
use futures::Sink;
use futures::executor::spawn;
use futures::stream::Stream;
use std::error::Error;
use futures_cpupool::CpuPool;

pub struct AutomaticCactus {
    hls: Arc<RwLock<Hls>>,
    cpu_pool: CpuPool,
}

impl AutomaticCactus {
    pub fn new(hls: Arc<RwLock<Hls>>, cpu_pool: CpuPool) -> AutomaticCactus {
        AutomaticCactus {
            hls: hls,
            cpu_pool: cpu_pool,
        }
    }
}

impl Service for AutomaticCactus {
    type Request = Request;
    type Error = hyper::Error;
    type Response = Response<LazyBytesStream>;
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
                        Some(segment) => Response::new()
                            .with_body(LazyBytesStream::new(segment)),
                        _ => Response::new().with_status(StatusCode::NotFound),
                    },
                    Err(err) => {
                        let body = format!("Invalid segment index: {}", err.description());
                        Response::new()
                            .with_header(ContentLength(body.len() as u64))
                            .with_status(StatusCode::BadRequest)
                            .with_body(LazyBytesStream::from(body))
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
                Response::new()
                    .with_header(ContentLength(playlist.len() as u64))
                    .with_header(ContentType(content_type))
                    .with_body(LazyBytesStream::from(playlist))
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
                            Ok(_) => Response::new()
                                .with_header(ContentLength(buf.len() as u64))
                                .with_body(LazyBytesStream::from(buf)),
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
