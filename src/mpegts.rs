use bytes::Bytes;
use std::os::raw::c_int;
use std::slice::from_raw_parts;
use std::ptr::null_mut;
use ffmpeg_sys::*;
use libc;
use std::ffi::CString;

pub struct MpegTs {
    output_format: *mut AVFormatContext,
    output_io: *mut AVIOContext,
    output_video_stream: *mut AVStream,
    output: Box<Output>,
}

impl Drop for MpegTs {
    fn drop(&mut self) {
        unsafe {
            if !self.output_format.is_null() {
                avformat_free_context(self.output_format);
                self.output_format = null_mut();
            }
            if !self.output_io.is_null() {
                av_free(self.output_io as *mut libc::c_void);
                self.output_io = null_mut();
            }
            if !self.output_video_stream.is_null() {
                self.output_video_stream = null_mut();
            }
        }
    }
}

struct Output {
    data: Bytes,
}

unsafe extern "C" fn write_output(
    opaque: *mut libc::c_void,
    input_buf: *mut u8,
    input_buf_size: c_int,
) -> c_int {
    let output = &mut *(opaque as *mut Output);
    output
        .data
        .extend_from_slice(from_raw_parts(input_buf, input_buf_size as usize));
    input_buf_size
}

fn default_av_packet() -> AVPacket {
    AVPacket {
        buf: null_mut(),
        data: null_mut(),
        pts: 0,
        dts: 0,
        duration: 0,
        convergence_duration: 0,
        flags: 0,
        pos: 0,
        side_data: null_mut(),
        stream_index: 0,
        size: 0,
        side_data_elems: 0,
    }
}

impl MpegTs {
    pub unsafe fn new(width: usize, height: usize) -> MpegTs {
        const AVIO_CTX_BUFFER_SIZE: usize = 8192;

        let mut obj = MpegTs {
            output_format: null_mut(),
            output_io: null_mut(),
            output: Box::new(Output { data: Bytes::new() }),
            output_video_stream: null_mut(),
        };
        let output_file_name =
            CString::new("output.ts").expect("Oops! can't parse output file name");
        let mut r = avformat_alloc_output_context2(
            &mut obj.output_format,
            null_mut(),
            null_mut(),
            output_file_name.as_ptr(),
        );
        if r < 0 {
            panic!("Failed to alloc output context: {}", r)
        }

        let output_io_buf = av_mallocz(AVIO_CTX_BUFFER_SIZE) as *mut u8;
        if output_io_buf.is_null() {
            panic!("Failed to alloc output io buf");
        }

        obj.output_io = avio_alloc_context(
            output_io_buf,
            AVIO_CTX_BUFFER_SIZE as i32,
            1,
            obj.output.as_mut() as *mut Output as *mut libc::c_void,
            None,
            Some(write_output),
            None,
        );
        if obj.output_io.is_null() {
            panic!("Failed to alloc output io");
        }

        (*(obj.output_format)).pb = obj.output_io;

        obj.output_video_stream = avformat_new_stream(obj.output_format, null_mut());
        if obj.output_video_stream.is_null() {
            panic!("Failed to allocate new video stream")
        }
        // (*output_video_stream).time_base = (*input_stream).time_base;
        let codecpar = &mut *(*obj.output_video_stream).codecpar;
        codecpar.codec_type = AVMediaType::AVMEDIA_TYPE_VIDEO;
        codecpar.codec_id = AVCodecID::AV_CODEC_ID_H264;
        codecpar.bits_per_raw_sample = 8;
        codecpar.profile = 578;
        codecpar.level = 41;
        codecpar.width = width as i32;
        codecpar.height = height as i32;
        codecpar.sample_aspect_ratio.den = 1;

        r = avformat_write_header(obj.output_format, null_mut());
        if r < 0 {
            panic!("Failed to write ts header");
        }

        obj
    }

    pub unsafe fn write(
        &mut self,
        h264: &mut Vec<u8>,
        start_ms: u64,
        frame_duration_ms: u64,
        key: bool,
    ) {
        let mut packet = default_av_packet();

        if key {
            packet.flags |= AV_PKT_FLAG_KEY;
        }
        let time_base = (*self.output_video_stream).time_base;
        let den = time_base.den as i64;
        let num = time_base.num as i64;
        packet.pts = start_ms as i64 * den / (num * 1000);
        packet.dts = packet.pts;
        packet.duration = frame_duration_ms as i64 * den / (num * 1000);
        packet.pos = -1;
        packet.stream_index = 0;
        packet.data = h264.as_mut_ptr();
        packet.size = h264.len() as i32;
        let r = av_interleaved_write_frame(self.output_format, &mut packet);
        if r < 0 {
            panic!("Failed to write video frame: {}", r)
        }
    }

    pub unsafe fn flush(&mut self) -> Bytes {
        av_write_trailer(self.output_format);
        self.output.data.clone()
    }
}
