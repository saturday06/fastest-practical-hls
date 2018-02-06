extern crate bytes;
extern crate chrono;
extern crate ffmpeg_sys;
extern crate futures;
extern crate hyper;
extern crate libc;
extern crate magick_rust;
extern crate openh264_sys;
extern crate tokio_core;
extern crate tokio_timer;

mod service;
mod hls;
mod camcoder;
mod mpegts;
mod lazybytes;

use hyper::server::Http;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use magick_rust::magick_wand_genesis;
use tokio_core::reactor::{Core, Interval};
use std::time::Duration;
use futures::Stream;
use ffmpeg_sys::av_register_all;

fn main() {
    std::process::exit({
        let camcoder_thread_stop_writer = Arc::new(AtomicBool::new(false));
        let camcoder_thread_stop_reader = camcoder_thread_stop_writer.clone();

        let camcoder_hls = hls::Hls::new();
        let server_hls = camcoder_hls.clone();

        let camcoder_thread = std::thread::spawn(move || {
            magick_wand_genesis();
            unsafe { av_register_all() };
            let ts_duration_ms = 350;
            let tick_ms = 50; // 20fps
            let mut camcoder =
                camcoder::Camcorder::new(camcoder_hls.clone(), tick_ms, ts_duration_ms);
            let mut core = Core::new().expect("Failed to allocate tokio_core::reactor::Core");
            let handle = core.handle();
            let interval_duration = Duration::from_millis(tick_ms);
            let interval = Interval::new(interval_duration, &handle).expect(&format!(
                "Failed to allocate interval: {:?}",
                interval_duration
            ));
            core.run(interval.for_each(|_| {
                if camcoder.run() && !camcoder_thread_stop_reader.as_ref().load(Ordering::Relaxed) {
                    futures::future::ok(())
                } else {
                    futures::future::ok(())
                    // futures::future::err(())
                }
            })).expect("Failed to run interval");
        });

        let addr_str = "127.0.0.1:3001";
        let addr = addr_str
            .parse()
            .expect(&format!("Failed to parse address {}", addr_str));
        //let static_path = Path::new(file!()).parent().map(|path|
        //    path.parnet().map(|path| path.join("www"))
        //).expect("todo: flatten").expect("what's");
        let server = Http::new()
            .bind(&addr, move || {
                Ok(service::AutomaticCactus::new(server_hls.clone()))
            })
            .expect(&format!("Failed to bind {:?}", addr));
        server
            .run()
            .expect(&format!("Failed to run server {:?}", addr));

        camcoder_thread_stop_writer
            .as_ref()
            .store(true, Ordering::Relaxed);
        camcoder_thread
            .join()
            .expect("Failed to join camcoder thread");

        0
    });
}
