use std::{env, process};

use grim_rs::{CaptureParameters, Grim};
use hyprland::{data::CursorPosition, prelude::*};
use raylib::{
    ffi::{Image as FfiImage, SetWindowMonitor, ToggleFullscreen},
    prelude::*,
};

const SPOTLIGHT_TINT: Color = Color::new(0x00, 0x00, 0x00, 190);

fn get_initial_cursor_pos_for_output(
    out_x: i32,
    out_y: i32,
    out_w: i32,
    out_h: i32,
) -> Option<Vector2> {
    let pos = CursorPosition::get().ok()?;

    let local_x = pos.x as f32 - out_x as f32;
    let local_y = pos.y as f32 - out_y as f32;

    Some(Vector2::new(
        local_x.clamp(0.0, out_w as f32),
        local_y.clamp(0.0, out_h as f32),
    ))
}

fn main() {
    let mut args = env::args();
    let bin = args.next().unwrap();

    let mut monitor_name: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--monitor" => {
                monitor_name = args.next().or_else(|| {
                    eprintln!("--monitor needs a value");
                    process::exit(1);
                })
            }
            _ => print_help_and_exit(&bin),
        }
    }

    let mut grim = Grim::new().expect("failed to initialize grim-rs");
    let outputs = grim.get_outputs().expect("failed to get outputs");

    if outputs.is_empty() {
        eprintln!("No Wayland outputs found.");
        process::exit(1);
    }

    let selected_output = match monitor_name {
        None => &outputs[0],
        Some(ref name) => outputs
            .iter()
            .find(|out| out.name() == name)
            .unwrap_or_else(|| {
                eprintln!("Output '{}' not found.", name);
                process::exit(1);
            }),
    };

    let mut spotlight_mouse_position = get_initial_cursor_pos_for_output(
        selected_output.geometry().x(),
        selected_output.geometry().y(),
        selected_output.geometry().width() as i32,
        selected_output.geometry().height() as i32,
    )
    .unwrap_or(Vector2::new(
        selected_output.geometry().width() as f32 * 0.5,
        selected_output.geometry().height() as f32 * 0.5,
    ));

    let params: Vec<CaptureParameters> = outputs
        .iter()
        .map(|out| CaptureParameters::new(out.name()).overlay_cursor(false))
        .collect();

    let results = grim
        .capture_outputs(params)
        .expect("failed to capture outputs");

    // Compute the bounding box of all outputs in layout space.
    let min_x = outputs
        .iter()
        .map(|o| o.geometry().x())
        .min()
        .expect("no outputs");
    let min_y = outputs
        .iter()
        .map(|o| o.geometry().y())
        .min()
        .expect("no outputs");
    let max_x = outputs
        .iter()
        .map(|o| o.geometry().x() + o.geometry().width() as i32)
        .max()
        .expect("no outputs");
    let max_y = outputs
        .iter()
        .map(|o| o.geometry().y() + o.geometry().height() as i32)
        .max()
        .expect("no outputs");

    let width = (max_x - min_x) as u32;
    let height = (max_y - min_y) as u32;

    // RGBA canvas
    let mut raw_pixels = vec![0u8; (width * height * 4) as usize];

    for output in outputs.iter() {
        let result = results
            .get(output.name())
            .expect("missing capture result for output");

        let ox = (output.geometry().x() - min_x) as u32;
        let oy = (output.geometry().y() - min_y) as u32;

        let out_w = result.width() as u32;
        let out_h = result.height() as u32;
        let src = result.data();

        for row in 0..out_h {
            let dst_start = (((oy + row) * width + ox) * 4) as usize;
            let dst_end = dst_start + (out_w * 4) as usize;

            let src_start = (row * out_w * 4) as usize;
            let src_end = src_start + (out_w * 4) as usize;

            raw_pixels[dst_start..dst_end].copy_from_slice(&src[src_start..src_end]);
        }
    }

    let (mut rl, thread) = raylib::init()
        .title(env!("CARGO_BIN_NAME"))
        // .size(width as i32, height as i32)
        .size(
            selected_output.geometry().width() as i32,
            selected_output.geometry().height() as i32,
        )
        .transparent()
        .undecorated()
        .vsync()
        .build();

    let idx = outputs
        .iter()
        .position(|o| o.name() == selected_output.name())
        .expect("Monitor not found");

    unsafe {
        ToggleFullscreen();
    }

    unsafe {
        SetWindowMonitor(idx as i32);
    }

    let screenshot_image = unsafe {
        Image::from_raw(FfiImage {
            // raylib frees this memory for us
            data: Box::new(raw_pixels).leak().as_mut_ptr().cast(),
            format: PixelFormat::PIXELFORMAT_UNCOMPRESSED_R8G8B8A8 as i32,
            mipmaps: 1,
            width: width as i32,
            height: height as i32,
        })
    };

    let screenshot_texture = rl
        .load_texture_from_image(&thread, &screenshot_image)
        .expect("failed to load screenshot into a texture");

    #[cfg(feature = "dev")]
    let mut spotlight_shader = rl
        .load_shader(&thread, None, Some("shaders/spotlight.fs"))
        .expect("Failed to load spotlight shader");

    #[cfg(not(feature = "dev"))]
    let mut spotlight_shader =
        rl.load_shader_from_memory(&thread, None, Some(include_str!("../shaders/spotlight.fs")));

    let mut rl_camera = Camera2D::default();
    rl_camera.zoom = 1.0;
    rl_camera.target = Vector2::new(
        (selected_output.geometry().x() - min_x) as f32,
        (selected_output.geometry().y() - min_y) as f32,
    );

    let mut delta_scale = 0f64;
    let mut scale_pivot = rl.get_mouse_position();
    let mut velocity = Vector2::default();
    let mut spotlight_radius_multiplier = 1.0;
    let mut spotlight_radius_multiplier_delta = 0.0;
    let mut spotlight_opacity = 0.0f32;

    #[cfg(feature = "dev")]
    let mut spotlight_tint_uniform_location;
    #[cfg(feature = "dev")]
    let mut cursor_position_uniform_location;
    #[cfg(feature = "dev")]
    let mut spotlight_radius_multiplier_uniform_location;
    #[cfg(not(feature = "dev"))]
    let spotlight_tint_uniform_location;
    #[cfg(not(feature = "dev"))]
    let cursor_position_uniform_location;
    #[cfg(not(feature = "dev"))]
    let spotlight_radius_multiplier_uniform_location;

    spotlight_tint_uniform_location = spotlight_shader.get_shader_location("spotlightTint");
    cursor_position_uniform_location = spotlight_shader.get_shader_location("cursorPosition");
    spotlight_radius_multiplier_uniform_location =
        spotlight_shader.get_shader_location("spotlightRadiusMultiplier");

    let idx = outputs
        .iter()
        .position(|o| o.name() == selected_output.name())
        .expect("Monitor not found");

    unsafe {
        ToggleFullscreen();
        SetWindowMonitor(idx as i32);
    }

    // Draw one fully-populated frame immediately so the first visible frame
    // is the screenshot, not an empty/unstyled window.
    {
        let mut d = rl.begin_drawing(&thread);
        let mut mode2d = d.begin_mode2D(rl_camera);
        mode2d.clear_background(Color::get_color(0));
        mode2d.draw_texture(&screenshot_texture, 0, 0, Color::WHITE);
    }

    let mut should_exit = false;
    while !rl.window_should_close() && !should_exit {
        if rl.is_key_pressed(KeyboardKey::KEY_Q) || rl.is_key_pressed(KeyboardKey::KEY_A) {
            should_exit = true;
        }

        if rl.is_mouse_button_down(MouseButton::MOUSE_BUTTON_RIGHT) {
            break;
        }

        #[cfg(feature = "dev")]
        if rl.is_key_pressed(KeyboardKey::KEY_R) {
            spotlight_shader = rl
                .load_shader(&thread, None, Some("shaders/spotlight.fs"))
                .expect("Failed to load spotlight shader");
            spotlight_tint_uniform_location = spotlight_shader.get_shader_location("spotlightTint");
            cursor_position_uniform_location =
                spotlight_shader.get_shader_location("cursorPosition");
            spotlight_radius_multiplier_uniform_location =
                spotlight_shader.get_shader_location("spotlightRadiusMultiplier");
        }

        let enable_spotlight = rl.is_key_down(KeyboardKey::KEY_LEFT_CONTROL)
            || rl.is_key_down(KeyboardKey::KEY_RIGHT_CONTROL);

        let scrolled_amount = rl.get_mouse_wheel_move_v().y;
        let frame_time = rl.get_frame_time();

        let target_opacity = if enable_spotlight {
            SPOTLIGHT_TINT.a as f32 / 255.0
        } else {
            0.0
        };

        let fade_speed = 4.0f32;
        spotlight_opacity += (target_opacity - spotlight_opacity) * frame_time * fade_speed;

        if rl.is_key_pressed(KeyboardKey::KEY_LEFT_CONTROL)
            || rl.is_key_pressed(KeyboardKey::KEY_RIGHT_CONTROL)
        {
            spotlight_radius_multiplier = 5.0;
            spotlight_radius_multiplier_delta = -15.0;
        }

        if scrolled_amount != 0.0 {
            match (
                enable_spotlight,
                rl.is_key_down(KeyboardKey::KEY_LEFT_SHIFT)
                    || rl.is_key_down(KeyboardKey::KEY_RIGHT_SHIFT),
            ) {
                (_, false) => {
                    delta_scale += scrolled_amount as f64;
                }
                (true, true) => {
                    spotlight_radius_multiplier_delta -= scrolled_amount as f64;
                }
                _ => {}
            }
            scale_pivot = rl.get_mouse_position();
        }

        if delta_scale.abs() > 0.5 {
            let p0 = scale_pivot / rl_camera.zoom;
            rl_camera.zoom = (rl_camera.zoom as f64 + delta_scale * frame_time as f64)
                .clamp(1.0, 10.0) as f32;
            let p1 = scale_pivot / rl_camera.zoom;
            rl_camera.target += p0 - p1;
            delta_scale -= delta_scale * frame_time as f64 * 4.0;
        }

        spotlight_radius_multiplier = (spotlight_radius_multiplier as f64
            + spotlight_radius_multiplier_delta * frame_time as f64)
            .clamp(0.3, 10.0) as f32;

        spotlight_radius_multiplier_delta -=
            spotlight_radius_multiplier_delta * frame_time as f64 * 4.0;

        const VELOCITY_THRESHOLD: f32 = 15.0;
        if rl.is_mouse_button_down(MouseButton::MOUSE_BUTTON_LEFT) {
            let delta = rl.get_screen_to_world2D(
                rl.get_mouse_position() - rl.get_mouse_delta(),
                rl_camera,
            ) - rl.get_screen_to_world2D(rl.get_mouse_position(), rl_camera);

            rl_camera.target += delta;
            velocity = delta * rl.get_fps().as_f32();
        } else if velocity.length_sqr() > VELOCITY_THRESHOLD * VELOCITY_THRESHOLD {
            rl_camera.target += velocity * frame_time;
            velocity -= velocity * frame_time * 6.0;
        }

        let mut d = rl.begin_drawing(&thread);
        let mut mode2d = d.begin_mode2D(rl_camera);

        if enable_spotlight || spotlight_opacity > 0.001 {
            mode2d.clear_background(Color::get_color(0));

            let current_mouse_position = mode2d.get_mouse_position();

            if current_mouse_position.x != 0.0 || current_mouse_position.y != 0.0 {
                spotlight_mouse_position = current_mouse_position;
            }

            let mouse_position = spotlight_mouse_position;

            spotlight_shader.set_shader_value(
                spotlight_tint_uniform_location,
                Vector4::new(
                    SPOTLIGHT_TINT.r as f32 / 255.0,
                    SPOTLIGHT_TINT.g as f32 / 255.0,
                    SPOTLIGHT_TINT.b as f32 / 255.0,
                    spotlight_opacity,
                ),
            );

            let screen_height = mode2d.get_screen_height().as_f32();
            spotlight_shader.set_shader_value(
                cursor_position_uniform_location,
                Vector2::new(mouse_position.x, screen_height - mouse_position.y),
            );

            spotlight_shader.set_shader_value(
                spotlight_radius_multiplier_uniform_location,
                spotlight_radius_multiplier,
            );

            let mut shader_mode = mode2d.begin_shader_mode(&mut spotlight_shader);
            shader_mode.draw_texture(&screenshot_texture, 0, 0, Color::WHITE);
        } else {
            mode2d.clear_background(Color::get_color(0));
            mode2d.draw_texture(&screenshot_texture, 0, 0, Color::WHITE);
        }
    }
}

fn print_help_and_exit(bin: &str) -> ! {
    eprintln!(
        "\
{bin}  – Wayland screen-zoom tool

USAGE:
    {bin} [--monitor <name>]

OPTIONS:
    --monitor <name>   Target monitor (Wayland output name); defaults to primary if flag is not provided.",
        bin = bin
    );
    process::exit(0);
}
