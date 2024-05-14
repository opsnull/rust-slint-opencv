use std::{
    sync::mpsc::{channel, Receiver, Sender},
    thread::{spawn, JoinHandle},
};

use anyhow::Result;
use opencv::{
    core::{self, MatTraitConst},
    imgproc::{cvt_color, COLOR_BGR2RGBA},
    prelude::*,
    videoio::{self, VideoCapture, VideoCaptureTrait},
};

use slint::{Image, Timer, TimerMode};

const CAMERA_INDEX: i32 = 0;

use slint::slint;
slint! {
    import {VerticalBox, HorizontalBox} from "std-widgets.slint";

export component Main inherits Window {
    title: "slint";
    icon: @image-url("");
    width: 1152px;
    height: 648px;

    pure callback render-image(int) -> image;
    in-out property <int> frame;

    VerticalLayout {
        HorizontalLayout {
            alignment: center;
            Rectangle {
                border-color: white;
                border-width: 1px;
                width: 1152px;
                height: 648px;
                Image {
                    width: 100%;
                    height: 100%;
                    source: render-image(frame);
                }
            }
        }
    }
}

}

fn main() -> Result<()> {
    // 打开摄像头
    let camera = VideoCapture::new(CAMERA_INDEX, videoio::CAP_ANY)?;
    let opened = VideoCapture::is_opened(&camera)?;
    if !opened {
        panic!("Unable to open default camera!");
    }
    // 获得摄像头参数
    let frame_width = camera.get(videoio::CAP_PROP_FRAME_WIDTH).unwrap();
    let frame_height = camera.get(videoio::CAP_PROP_FRAME_HEIGHT).unwrap();
    let fps = camera.get(videoio::CAP_PROP_FPS).unwrap();
    println!(
        "camera: width {}, height {}, FPS: {}",
        frame_width, frame_height, fps
    );

    let window = Main::new().unwrap();
    let window_clone = window.as_weak();

    let timer = Timer::default();
    timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_secs_f32(1. / (fps + 10.0) as f32), // fps + 10  是加快 slint 显示图片的频率, 显示的视频更流畅
        move || {
            if let Some(window) = window_clone.upgrade() {
                window.set_frame(window.get_frame() + 1);
            }
        },
    );

    // 创建 Sline 和 Camera image 之间的数据通道
    let (frame_sender, frame_receiver) = channel();
    // 优雅退出 channel, 确保文件和 camera 对象被正常关闭, 否则 mp4 文件不完整
    let (exit_sender, exit_receiver) = channel();

    let task = start(
        frame_sender,
        exit_receiver,
        camera,
        frame_width,
        frame_height,
        fps,
    );

    // 需要确保 frame_data 的大小和从摄像头的分辨率一致, 否则后续 copy_from_slice() 会报错.
    let mut frame_data = vec![0; (frame_width * frame_height * 4.0) as usize];
    let mut render = move || -> Result<Image> {
        if let Ok(frame_rgba) = frame_receiver.try_recv() {
            frame_data.copy_from_slice(&frame_rgba);
        }
        let v = slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
            frame_data.as_slice(),
            frame_width as u32,
            frame_height as u32,
        ));
        Ok(v)
    };

    window.on_render_image(move |_frame| render().map_err(|err| eprintln!("{:?}", err)).unwrap());
    // 阻塞, 直到窗口被关闭.
    window.run().unwrap();

    // 关闭摄像头和文件.
    exit_sender.send(())?;
    let result = task.join().unwrap();
    println!("Camera Stopped And File Closed {:?}", result);
    Ok(())
}

fn start(
    frame_sender: Sender<Vec<u8>>,
    exit_receiver: Receiver<()>,
    mut camera: VideoCapture,
    frame_width: f64,
    frame_height: f64,
    fps: f64,
) -> JoinHandle<Result<()>> {
    spawn(move || -> Result<()> {
        let fourcc = videoio::VideoWriter::fourcc('m', 'p', '4', 'v').unwrap();
        let mut out = videoio::VideoWriter::new(
            "test.mp4",
            fourcc,
            fps, // 需要和 camera FPS 一致, 播放保存的 mp4 视频才正常速度
            core::Size2i::new(frame_width as i32, frame_height as i32),
            true,
        )
        .expect("Can not open video writer");

        let mut frame_bgr = Mat::default();
        let mut frame_rgba = Mat::default();
        loop {
            if let Ok(()) = exit_receiver.try_recv() {
                break;
            } else {
                camera.read(&mut frame_bgr)?;

                // 需要转换称 Slint 显示的 RGBA 像素格式.
                cvt_color(&frame_bgr, &mut frame_rgba, COLOR_BGR2RGBA, 0)?;

                frame_sender.send(frame_rgba.data_bytes()?.to_vec())?;

                if frame_bgr.size().unwrap().width > 0 {
                    let _ = out.write(&frame_bgr);
                }

                //std::thread::sleep(Duration::from_millis(10));
            }
        }
        Ok(())
    })
}
