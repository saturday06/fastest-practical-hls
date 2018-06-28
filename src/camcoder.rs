use hls::Hls;
use std::sync::{Arc, RwLock, Mutex};
use std::ffi::CString;
use magick_rust::{DrawingWand, MagickWand, PixelWand};
use magick_rust::bindings::{ColorspaceType, DrawRectangle, GravityType, MagickBooleanType,
                            MagickDrawImage, MagickExportImagePixels, StorageType};
use chrono::prelude::*;
use libc;
use ffmpeg_sys::{sws_scale, AVPixelFormat, SwsContext, sws_getContext, SWS_FAST_BILINEAR};
use std::ptr::{null, null_mut, copy_nonoverlapping};
use std::os::raw::{c_int, c_void};
use openh264_sys::*;
use std::slice::from_raw_parts;
use mpegts::MpegTs;
use lazybytes::LazyBytes;
use bytes::Bytes;
use std::marker::Send;
use webrtcelevator;
use std::cell::RefCell;

pub struct Camcorder {
    hls: Arc<RwLock<Hls>>,
    magick_wand: MagickWand,
    text_drawing: DrawingWand,
    background_drawing: DrawingWand,
    magick_image_pixels: Vec<u8>,
    y_pixels: Vec<u8>,
    u_pixels: Vec<u8>,
    v_pixels: Vec<u8>,
    y_stride: usize,
    u_stride: usize,
    v_stride: usize,
    width: usize,
    height: usize,
    sws_context: *mut SwsContext,
    svc_encoder: *mut *const ISVCEncoderVtbl,
    frame_duration_ms: u64,
    current_ms: u64,
    ts_duration_ms: u64,
    mpeg_ts: MpegTs,
    h264: Vec<u8>,
    intra_count: usize,
}

unsafe impl Send for Camcorder {
}

pub unsafe extern "C" fn frame_callback(target_ptr: *mut c_void, frame_ptr: *const webrtcelevator::webrtc_elevator_video_frame) {
    let camcoder_arc = (target_ptr as *mut Arc<Mutex<Camcorder>>).as_ref().expect("camcoder");
    let frame = frame_ptr.as_ref().expect("frame ref");
    let y_len = frame.width * frame.height;
    let uv_width = (frame.width + 1) / 2;
    let uv_len = uv_width * ((frame.height + 1) / 2);

    let mut camcoder = camcoder_arc.lock().expect("lock camcoder");
    
    println!("I'm called from C with value {}x{} y={} uv={}", frame.width, frame.height, y_len, uv_len);

    if camcoder.y_pixels.len() < y_len {
        camcoder.y_pixels.resize(y_len, 0);
    }
    if camcoder.u_pixels.len() < uv_len {
        camcoder.u_pixels.resize(uv_len, 0);
    }
    if camcoder.v_pixels.len() < uv_len {
        camcoder.y_pixels.resize(uv_len, 0);
    }
    camcoder.y_stride = frame.width;
    camcoder.u_stride = uv_width;
    camcoder.v_stride = uv_width;

    copy_nonoverlapping(frame.y, camcoder.y_pixels.as_mut_ptr(), y_len);
    copy_nonoverlapping(frame.u, camcoder.u_pixels.as_mut_ptr(), uv_len);
    copy_nonoverlapping(frame.v, camcoder.v_pixels.as_mut_ptr(), uv_len);
    
    /*
    unsafe {
        // コールバックから受け取った値でRustObjectの中の値をアップデートする
        (*target).a = a;
    }*/
}

impl Camcorder {
    pub fn new(hls: Arc<RwLock<Hls>>, frame_duration_ms: u64, ts_duration_ms: u64) -> Camcorder {
        let width = 480;
        let height = 320;
        let mut text_fill_color = PixelWand::new();
        text_fill_color
            .set_color("white")
            .expect("Failed to set text fill color");
        let mut text_drawing = DrawingWand::new();
        text_drawing.set_font_size(64.0);
        text_drawing.set_gravity(GravityType::CenterGravity);
        text_drawing.set_fill_color(&text_fill_color);
        let mut background_color = PixelWand::new();
        background_color
            .set_color("black")
            .expect("Failed to set background color");
        let mut background_drawing = DrawingWand::new();
        background_drawing.set_gravity(GravityType::CenterGravity);
        background_drawing.set_fill_color(&background_color);
        unsafe {
            DrawRectangle(
                background_drawing.wand,
                0.0,
                0.0,
                width as f64,
                height as f64,
            )
        };
        let mut magick_wand = MagickWand::new();
        magick_wand
            .new_image(width, height, &background_color)
            .expect("Failed to create new image");
        magick_wand
            .set_image_colorspace(ColorspaceType::sRGBColorspace)
            .expect("Failed to set colorspace");
        magick_wand
            .set_image_depth(8)
            .expect("Failed to set bit depth");
        let mut magick_image_pixels = Vec::new();
        magick_image_pixels.resize(width * height * 3, 0);

        let y_stride = (width + 15) / 16 * 16;
        let mut y_pixels = Vec::new();
        y_pixels.resize(y_stride * height, 0);

        let u_stride = (width + 15) / 16 * 8;
        let mut u_pixels = Vec::new();
        u_pixels.resize(u_stride * height, 0);

        let v_stride = (width + 15) / 16 * 8;
        let mut v_pixels = Vec::new();
        v_pixels.resize(v_stride * height, 0);

        let sws_context = unsafe {
            sws_getContext(
                width as i32,
                height as i32,
                AVPixelFormat::AV_PIX_FMT_RGB24,
                width as i32,
                height as i32,
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                SWS_FAST_BILINEAR,
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if sws_context.is_null() {
            panic!("Oops! sws context is null")
        }

        let mut svc_encoder = null_mut();
        let mut r = unsafe { WelsCreateSVCEncoder(&mut svc_encoder) };
        if r != 0 {
            panic!("WelsCreateSVCEncoder: {}", r);
        }
        if svc_encoder.is_null() {
            panic!("svc encoder is null");
        }

        let mut param = SEncParamExt::default();
        r = unsafe { (**svc_encoder).GetDefaultParams.unwrap()(svc_encoder, &mut param) };
        if r != 0 {
            panic!("GetDefaultParams: {}", r);
        }

        let fps = 1000.0 / frame_duration_ms as f32;
        let bitrate = 4000000;
        param.iUsageType = CAMERA_VIDEO_REAL_TIME;
        param.fMaxFrameRate = fps;
        param.iMaxBitrate = UNSPECIFIED_BIT_RATE as i32;
        param.iSpatialLayerNum = 1; // layer number at spatial level
        param.bSimulcastAVC = true;
        param.iMultipleThreadIdc = 4;
        param.sSpatialLayers[0].uiProfileIdc = PRO_BASELINE;
        param.sSpatialLayers[0].iVideoWidth = width as i32;
        param.sSpatialLayers[0].iVideoHeight = height as i32;
        param.sSpatialLayers[0].fFrameRate = fps;
        param.sSpatialLayers[0].iSpatialBitrate = bitrate;
        param.sSpatialLayers[0].iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE as i32;
        param.sSpatialLayers[0].sSliceArgument.uiSliceMode = SM_FIXEDSLCNUM_SLICE;
        param.sSpatialLayers[0].sSliceArgument.uiSliceNum = 4;
        param.iPicWidth = width as i32;
        param.iPicHeight = height as i32;
        param.iTargetBitrate = bitrate;

        r = unsafe { (**svc_encoder).InitializeExt.unwrap()(svc_encoder, &mut param) };
        if r != 0 {
            panic!("InitializeExt: {}", r);
        }

        let mut video_format = videoFormatI420 as c_int;
        r = unsafe {
            (**svc_encoder).SetOption.unwrap()(
                svc_encoder,
                ENCODER_OPTION_DATAFORMAT,
                &mut video_format as *mut c_int as *mut c_void,
            )
        };
        if r != 0 {
            panic!("SetOption: {}", r);
        }

        let mut rc_frame_skip = 0 as c_int;
        r = unsafe {
            (**svc_encoder).SetOption.unwrap()(
                svc_encoder,
                ENCODER_OPTION_RC_FRAME_SKIP,
                &mut rc_frame_skip as *mut c_int as *mut c_void,
            )
        };
        if r != 0 {
            panic!("SetOption: {}", r);
        }

        let lazy_bytes = Arc::new(RwLock::new(LazyBytes {
            bytes: Bytes::new(),
            completion: false,
        }));
        {
            let mut h = hls.write().expect("Failed to lock hls segments");
            h.add_new_segment(ts_duration_ms, lazy_bytes.clone());
        }

        Camcorder {
            magick_wand,
            text_drawing,
            background_drawing,
            magick_image_pixels,
            hls,
            width,
            height,
            y_pixels,
            u_pixels,
            v_pixels,
            y_stride,
            u_stride,
            v_stride,
            sws_context,
            svc_encoder,
            frame_duration_ms,
            current_ms: 0,
            ts_duration_ms,
            mpeg_ts: unsafe { MpegTs::new(width, height, lazy_bytes) },
            h264: Vec::new(),
            intra_count: 0,
        }
    }

    pub fn run(&mut self) -> bool {
        let force_intra_frame = if self.current_ms % self.ts_duration_ms == 0 {
            true
        } else {
            false
        };
        self.current_ms += self.frame_duration_ms;
        /*        
        let now = Local::now();
        let text = now.format("%Y-%m-%d\n%H:%M:%S\n%f").to_string();
        if unsafe { MagickDrawImage(self.magick_wand.wand, self.background_drawing.wand) }
            == MagickBooleanType::MagickFalse
        {
            panic!("Failed to draw background image");
        };
        self.magick_wand
            .annotate_image(&self.text_drawing, 0.0, 0.0, 0.0, &text)
            .expect("Failed to write text to image");
        let rgb = CString::new("RGB")
            .expect("Oops! invalid CString?")
            .into_bytes_with_nul();
        if unsafe {
            MagickExportImagePixels(
                self.magick_wand.wand,
                0,
                0,
                self.width,
                self.height,
                rgb.as_ptr() as *const i8,
                StorageType::CharPixel,
                self.magick_image_pixels.as_mut_ptr() as *mut libc::c_void,
            )
        } == MagickBooleanType::MagickFalse
        {
            panic!("Failed to get image pixels");
        }

        let src: [*const u8; 4] = [self.magick_image_pixels.as_ptr(), null(), null(), null()];
        let src_strides: [c_int; 4] = [self.width as i32 * 3, 0, 0, 0];
        let dst: [*const u8; 4] = [
            self.y_pixels.as_ptr(),
            self.u_pixels.as_ptr(),
            self.v_pixels.as_ptr(),
            null(),
        ];
        let dst_strides: [c_int; 4] = [
            self.y_stride as i32,
            self.u_stride as i32,
            self.v_stride as i32,
            0,
        ];
        if unsafe {
            sws_scale(
                self.sws_context,
                src.as_ptr(),
                src_strides.as_ptr(),
                0,
                self.height as i32,
                dst.as_ptr(),
                dst_strides.as_ptr(),
            )
        } == 0
        {
            panic!("Failed to execute sws_scale");
        }
         */

        let mut info = SFrameBSInfo::default();
        let mut pic = SSourcePicture::default();
        pic.uiTimeStamp = self.current_ms as i64;
        pic.iPicWidth = self.width as i32;
        pic.iPicHeight = self.height as i32;
        pic.iColorFormat = videoFormatI420 as i32;
        pic.iStride[0] = self.y_stride as i32;
        pic.iStride[1] = self.u_stride as i32;
        pic.iStride[2] = self.v_stride as i32;
        pic.pData[0] = self.y_pixels.as_mut_ptr();
        pic.pData[1] = self.u_pixels.as_mut_ptr();
        pic.pData[2] = self.v_pixels.as_mut_ptr();

        if force_intra_frame {
            self.intra_count += 1;
            if self.intra_count < 90 || self.intra_count % 10 == 0 {
                let r =
                    unsafe { (**self.svc_encoder).ForceIntraFrame.unwrap()(self.svc_encoder, true) };
                if r != 0 {
                    panic!("ForceIntraFrame: {}", r);
                }
            }
        }

        let r = unsafe {
            (**self.svc_encoder).EncodeFrame.unwrap()(self.svc_encoder, &mut pic, &mut info)
        };
        if r != 0 {
            if r != 0 {
                panic!("EncodeFrame: {}", r);
            }
        }

        if info.eFrameType == videoFrameTypeSkip {
            eprintln!("skip frame")
        } else if info.eFrameType == videoFrameTypeInvalid {
            eprintln!("inval")
        } else if info.eFrameType == videoFrameTypeIDR || info.eFrameType == videoFrameTypeI
            || info.eFrameType == videoFrameTypeP
            || info.eFrameType == videoFrameTypeIPMixed
        {
        } else {
            eprintln!("unknown frame: {:?}", info.eFrameType)
        }

        for spatial_id in 0..1 {
            for layer in 0..info.iLayerNum {
                if info.sLayerInfo[layer as usize].uiSpatialId != spatial_id {
                    continue;
                }

                let mut size = 0;
                for i in 0..info.sLayerInfo[layer as usize].iNalCount {
                    size += unsafe {
                        *info.sLayerInfo[layer as usize]
                            .pNalLengthInByte
                            .offset(i as isize)
                    };
                }
                if size > 0 {
                    unsafe {
                        self.h264.extend_from_slice(from_raw_parts(
                            info.sLayerInfo[layer as usize].pBsBuf,
                            size as usize,
                        ))
                    };
                }
            }
        }

        unsafe {
            self.mpeg_ts.write(
                &mut self.h264,
                self.current_ms - self.frame_duration_ms,
                self.frame_duration_ms,
                force_intra_frame,
            )
        };
        self.h264.clear();

        if self.current_ms % self.ts_duration_ms != 0 {
            return true;
        }

        unsafe { self.mpeg_ts.flush() };

        let lazy_bytes = Arc::new(RwLock::new(LazyBytes {
            bytes: Bytes::new(),
            completion: false,
        }));
        {
            let mut hls = self.hls.write().expect("Failed to lock hls segments");
            hls.add_new_segment(self.ts_duration_ms, lazy_bytes.clone());
        }

        self.mpeg_ts = unsafe { MpegTs::new(self.width, self.height, lazy_bytes.clone()) };
        /*
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("out.h264")
            .expect("open!");
        file.write_all(&self.h264).expect("write!");
        */
        return true;
    }
}
