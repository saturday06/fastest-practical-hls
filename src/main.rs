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
mod webrtcelevator;

use hyper::server::Http;
use std::sync::atomic::{AtomicBool, Ordering};
use magick_rust::magick_wand_genesis;
use tokio_core::reactor::{Core, Interval};
use std::time::Duration;
use futures::Stream;
use ffmpeg_sys::av_register_all;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::os::raw::c_void;
use std::ptr::null_mut;

fn main() {
    std::process::exit({
        let camcoder_thread_stop_writer = Arc::new(AtomicBool::new(false));
        let camcoder_thread_stop_reader = camcoder_thread_stop_writer.clone();

        let camcoder_hls = hls::Hls::new();
        let server_hls = camcoder_hls.clone();

        let camcoder_thread = std::thread::spawn(move || {
            magick_wand_genesis();
            unsafe { av_register_all() };
            let ts_duration_ms = 300;
            let tick_ms = 100; // 10fps
            let mut camcoder =
                Arc::new(Mutex::new(RefCell::new(camcoder::Camcorder::new(camcoder_hls.clone(), tick_ms, ts_duration_ms))));
            let mut core = Core::new().expect("Failed to allocate tokio_core::reactor::Core");
            let handle = core.handle();
            let interval_duration = Duration::from_millis(tick_ms);
            let interval = Interval::new(interval_duration, &handle).expect(&format!(
                "Failed to allocate interval: {:?}",
                interval_duration
            ));

            let mut callback_camcoder = camcoder.clone();
            std::thread::spawn(move || {
                unsafe {
                    webrtcelevator::start_webrtc_elevator(&mut callback_camcoder as *mut Arc<_> as *mut c_void, Some(camcoder::frame_callback));
                }
            });

            core.run(interval.for_each(|_| {
                let locked = camcoder.lock().expect("lock");
                if locked.borrow_mut().run() && !camcoder_thread_stop_reader.as_ref().load(Ordering::Relaxed) {
                    futures::future::ok(())
                } else {
                    futures::future::ok(())
                    // futures::future::err(())
                }
            })).expect("Failed to run interval");
        });

        let addr_str = "0.0.0.0:3000";
        let addr = addr_str
            .parse()
            .expect(&format!("Failed to parse address {}", addr_str));
        let server = Http::new()
            .bind(&addr, move || {
                Ok(service::HlsService::new(server_hls.clone()))
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
