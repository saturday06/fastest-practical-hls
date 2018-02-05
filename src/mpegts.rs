use bytes::Bytes;
use std::os::raw::{c_int, c_uchar};
use std::slice::{from_raw_parts, from_raw_parts_mut};
use std::cmp::min;
use std::ptr::null_mut;
use ffmpeg_sys::*;
use libc;
use std::ffi::CString;

pub struct MpegTs {
    frame_duration_ms: i32,
    input_video_format: *mut AVFormatContext,
    input_video_io: *mut AVIOContext,
    input_video_io_buf: *mut c_uchar,
    output_format: *mut AVFormatContext,
    output_io: *mut AVIOContext,
    output_io_buf: *mut c_uchar,
    packet: AVPacket,
}

struct Input<'a> {
    data: &'a Vec<u8>,
    offset: usize,
}

unsafe extern "C" fn read_input(
    opaque: *mut libc::c_void,
    output_buf: *mut u8,
    output_buf_size: c_int,
) -> c_int {
    let input = &mut *(opaque as *mut Input);
    let end = min(input.data.len(), input.offset + output_buf_size as usize);
    let written = end - input.offset;
    let output = from_raw_parts_mut(output_buf, written);
    output.copy_from_slice(&input.data[input.offset..end]);
    input.offset = end;
    written as i32
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
    pub fn new(frame_duration_ms: i32) -> MpegTs {
        MpegTs {
            frame_duration_ms: frame_duration_ms,
            input_video_format: null_mut(),
            input_video_io: null_mut(),
            input_video_io_buf: null_mut(),
            output_format: null_mut(),
            output_io: null_mut(),
            output_io_buf: null_mut(),
            packet: default_av_packet(),
        }
    }

    pub unsafe fn create(&mut self, h264: &Vec<u8>, start_ms: u64) -> Bytes {
        const AVIO_CTX_BUFFER_SIZE: usize = 8192;

        let mut input = Input {
            data: h264,
            offset: 0,
        };

        let mut output = Output { data: Bytes::new() };

        self.input_video_format = avformat_alloc_context();
        if self.input_video_format.is_null() {
            panic!("Failed to call avformat_alloc_context() for input video format");
        }

        self.input_video_io_buf = av_mallocz(AVIO_CTX_BUFFER_SIZE) as *mut u8;
        if self.input_video_io_buf.is_null() {
            panic!("Failed to alloc input video io buf");
        }

        self.input_video_io = avio_alloc_context(
            self.input_video_io_buf,
            AVIO_CTX_BUFFER_SIZE as i32,
            0,
            &mut input as *mut Input as *mut libc::c_void,
            Some(read_input),
            None,
            None,
        );
        if self.input_video_io.is_null() {
            panic!("Failed to alloc input video io");
        }
        (*self.input_video_format).pb = self.input_video_io;
        let input_video_file_name =
            CString::new("input.h264").expect("Oops! can't parse input file name");

        let mut r = avformat_open_input(
            &mut self.input_video_format,
            input_video_file_name.as_ptr() as *const i8,
            null_mut(),
            null_mut(),
        );
        if r < 0 {
            panic!("Failed to open input video: {}", r)
        }

        r = avformat_find_stream_info(self.input_video_format, null_mut());
        if r < 0 {
            panic!("Failed to find video stream info: {}", r)
        }

        let output_file_name =
            CString::new("output.ts").expect("Oops! can't parse output file name");
        r = avformat_alloc_output_context2(
            &mut self.output_format,
            null_mut(),
            null_mut(),
            output_file_name.as_ptr(),
        );
        if r < 0 {
            panic!("Failed to alloc output context: {}", r)
        }

        self.output_io_buf = av_mallocz(AVIO_CTX_BUFFER_SIZE) as *mut u8;
        if self.output_io_buf.is_null() {
            panic!("Failed to alloc output io buf");
        }

        self.output_io = avio_alloc_context(
            self.output_io_buf,
            AVIO_CTX_BUFFER_SIZE as i32,
            1,
            &mut output as *mut Output as *mut libc::c_void,
            None,
            Some(write_output),
            None,
        );
        if self.output_io.is_null() {
            panic!("Failed to alloc output io");
        }

        (*self.output_format).pb = self.output_io;

        let mut output_video_stream_index_opt = None;
        let mut input_video_stream_index_opt = None;
        for i in 0..(*self.input_video_format).nb_streams as isize {
            let input_stream = *((*self.input_video_format).streams.offset(i));
            if (*((*input_stream).codecpar)).codec_type != AVMediaType::AVMEDIA_TYPE_VIDEO {
                continue;
            }
            let output_stream = avformat_new_stream(self.output_format, null_mut());
            if output_stream.is_null() {
                panic!("Failed to allocate new video stream")
            }
            input_video_stream_index_opt = Some((*input_stream).index);
            output_video_stream_index_opt = Some((*output_stream).index);
            r = avcodec_parameters_copy((*output_stream).codecpar, (*input_stream).codecpar);
            if r < 0 {
                panic!(
                    "Failed to copy parameters to stream {:?}: {}",
                    output_video_stream_index_opt, r
                );
            }
            (*output_stream).time_base = (*input_stream).time_base;
            break;
        }
        let input_video_stream_index =
            input_video_stream_index_opt.expect("Failed to find video input stream index");
        let output_video_stream_index =
            output_video_stream_index_opt.expect("Failed to find video output stream index");

        r = avformat_write_header(self.output_format, null_mut());
        if r < 0 {
            panic!("Failed to write ts header");
        }

        let mut current_ms = start_ms as i64;
        loop {
            let output_stream = *((*self.output_format)
                .streams
                .offset(output_video_stream_index as isize));
            let output_video_time_base = (*output_stream).time_base;

            r = av_read_frame(self.input_video_format, &mut self.packet);
            if r == AVERROR_EOF {
                break;
            } else if r < 0 {
                panic!("Failed to read video frame: {}", r)
            }
            if self.packet.stream_index != input_video_stream_index {
                continue;
            }

            let den = output_video_time_base.den as i64;
            let num = output_video_time_base.num as i64;
            self.packet.pts = current_ms * den / (num * 1000);
            self.packet.dts = self.packet.pts;
            self.packet.duration = self.frame_duration_ms as i64 * den / (num * 1000);
            self.packet.pos = -1;
            self.packet.stream_index = output_video_stream_index;
            r = av_interleaved_write_frame(self.output_format, &mut self.packet);
            if r < 0 {
                panic!("Failed to write video frame: {}", r)
            }
            av_packet_unref(&mut self.packet);

            current_ms += self.frame_duration_ms as i64;
        }

        av_write_trailer(self.output_format);

        output.data
    }
}
