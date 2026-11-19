//! Drawing a model off-screen. Pictures from a graphics card aren't compared pixel for pixel:
//! these check where a model lands, its colours within a tolerance, and that a known still is
//! close to a stored one on the software adapter. Each test that draws skips, saying so, where
//! this computer has no adapter at all.

use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use slides_model::*;

/// A drawer that, once a test is done with it, waits for its thread to let go of its device:
/// the program ending while a driver lets go of one can crash in the driver.
struct Closing(Option<Drawer>);

impl std::ops::Deref for Closing {
    type Target = Drawer;

    fn deref(&self) -> &Drawer {
        self.0.as_ref().expect("not yet closed")
    }
}

impl Drop for Closing {
    fn drop(&mut self) {
        if let Some(drawer) = self.0.take() {
            assert!(drawer.close(Duration::from_secs(30)), "the drawer's thread never ended");
        }
    }
}

fn start(options: DrawerOptions) -> Closing {
    Closing(Some(Drawer::start_with(options)))
}

/// Waits for the drawer to say whether it can draw.
fn wait_ready(drawer: &Drawer) -> Result<AdapterKind, CantDraw> {
    let start = Instant::now();
    loop {
        if let Some(ready) = drawer.ready() {
            return ready;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "the drawer never said whether it can draw");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A drawer on the software adapter where there is one, else on any, else `None` after saying
/// why the test is skipped.
fn drawer(test: &str) -> Option<Closing> {
    drawer_with(test, DrawerOptions::default())
}

fn drawer_with(test: &str, options: DrawerOptions) -> Option<Closing> {
    let software = start(DrawerOptions { only: Some(AdapterKind::Software), ..options.clone() });
    if wait_ready(&software).is_ok() {
        return Some(software);
    }
    let any = start(options);
    match wait_ready(&any) {
        Ok(_) => Some(any),
        Err(why) => {
            eprintln!("{test} skipped: no graphics adapter, not even a software one, can be had here. {why}");
            None
        }
    }
}

fn model(bytes: Vec<u8>) -> Model {
    read(Box::new(Cursor::new(bytes))).expect("a made model reads")
}

fn boxes() -> Model {
    model(make_test_glb(MadeGlb::Boxes))
}

fn picture(receiver: Answer) -> Result<Picture, NotDrawn> {
    receiver.recv_timeout(Duration::from_secs(60)).expect("the drawer answered")
}

fn alpha(picture: &Picture, x: u32, y: u32) -> u8 {
    picture.bgra[((y * picture.width + x) * 4 + 3) as usize]
}

/// The pixel at `x`, `y` as RGBA.
fn rgba(picture: &Picture, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * picture.width + x) * 4) as usize;
    let p = &picture.bgra[at..at + 4];
    [p[2], p[1], p[0], p[3]]
}

/// The box around the pixels the model covers, as left, top, right and bottom edges.
fn covered(picture: &Picture) -> Option<(u32, u32, u32, u32)> {
    let mut edges: Option<(u32, u32, u32, u32)> = None;
    for y in 0..picture.height {
        for x in 0..picture.width {
            if alpha(picture, x, y) > 127 {
                let (l, t, r, b) = edges.unwrap_or((x, y, x + 1, y + 1));
                edges = Some((l.min(x), t.min(y), r.max(x + 1), b.max(y + 1)));
            }
        }
    }
    edges
}

/// Where a point lands in a picture `width` by `height` seen by a camera at `eye` looking
/// along -z: worked out here from the field of view alone, not by the drawer's own arithmetic.
fn projected(eye: [f32; 3], point: [f32; 3], width: u32, height: u32) -> (f32, f32) {
    let depth = eye[2] - point[2];
    let scale = 1.0 / (View::FIELD_OF_VIEW / 2.0).to_radians().tan() * width.min(height) as f32 / 2.0;
    (width as f32 / 2.0 + (point[0] - eye[0]) / depth * scale, height as f32 / 2.0 - (point[1] - eye[1]) / depth * scale)
}

#[test]
fn a_box_drawn_from_the_front_is_where_it_should_be() {
    let Some(drawer) = drawer("a_box_drawn_from_the_front_is_where_it_should_be") else { return };
    let scene = drawer.upload(boxes());
    for (width, height) in [(320, 180), (150, 200)] {
        let view = View { yaw: 0.0, pitch: 0.0, distance: 4.0, target: [0.0, 0.5, 0.0] };
        let picture = picture(drawer.draw(&scene, view, width, height)).expect("drawn");
        assert_eq!((picture.width, picture.height), (width, height));
        assert_eq!(picture.bgra.len(), (width * height * 4) as usize);
        // From straight in front only the box's front face, at z 0.5, can be seen.
        let eye = [0.0, 0.5, 4.0];
        let (left, top) = projected(eye, [-0.5, 1.0, 0.5], width, height);
        let (right, bottom) = projected(eye, [0.5, 0.0, 0.5], width, height);
        let (l, t, r, b) = covered(&picture).expect("the box covers something");
        for (got, want) in [(l, left), (t, top), (r, right), (b, bottom)] {
            assert!((got as f32 - want).abs() <= 2.0, "{width} by {height}: covered {:?}, expected {:?}", (l, t, r, b), (left, top, right, bottom));
        }
        // Opaque where the model is and clear elsewhere.
        let (cx, cy) = (width / 2, height / 2);
        assert_eq!(alpha(&picture, cx, cy), 255);
        assert_eq!(&picture.bgra[0..4], &[0, 0, 0, 0]);
        // The front face is the one looking along z, lit but still its own colour: blue the most.
        let [red, green, blue, _] = rgba(&picture, cx, cy);
        assert!(blue > red && blue > green, "the front face is {:?}", [red, green, blue]);
    }
}

#[test]
fn a_view_reads_back_as_written() {
    let view = View { yaw: 30.0, pitch: 15.0, distance: 4.5, target: [0.0, 0.5, -1.0] };
    assert_eq!(view.to_string(), "30 15 4.5 0 0.5 -1");
    assert_eq!(View::parse("30 15 4.5 0 0.5 -1"), Some(view));
    assert_eq!(View::parse("  30   15 4.5\t0 0.5 -1 "), Some(view));
    for view in [
        View { yaw: -12.345_678, pitch: 0.1, distance: 1e-3, target: [1e6, -0.0, 3.25] },
        View::framing(&boxes().facts),
        View { yaw: 359.99, pitch: -89.0, distance: 12345.678, target: [0.1, 0.2, 0.3] },
    ] {
        let written = view.to_string();
        assert_eq!(written.split(' ').count(), 6, "{written}");
        assert!(!written.contains("-0 ") && !written.starts_with("-0 ") && !written.ends_with(" -0"), "{written}");
        let back = View::parse(&written).expect("reads back");
        assert_eq!(back.to_string(), written);
        assert_eq!(back.yaw, view.yaw);
        assert_eq!(back.distance, view.distance);
    }
    // A view that isn't made of numbers is written as one that is.
    let nonsense = View { yaw: f32::NAN, pitch: f32::INFINITY, distance: -1.0, target: [f32::NAN, 1.0, f32::NEG_INFINITY] };
    assert_eq!(nonsense.to_string(), "0 0 1 0 1 0");
    let between = View::between(nonsense, View { yaw: 90.0, ..nonsense }, 0.5);
    assert!(View::parse(&between.to_string()).is_some(), "{between}");
    for bad in [
        "",
        "1 2 3",
        "1 2 3 4 5 6 7",
        "a b c d e f",
        "0 0 0 0 0 0",
        "0 0 -1 0 0 0",
        "NaN 0 1 0 0 0",
        "0 0 inf 0 0 0",
        "0 0 1 0 0 1e40",
        "0 0 1 0 0 x",
    ] {
        assert_eq!(View::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn framing_shows_the_whole_model() {
    let model = boxes();
    let view = View::framing(&model.facts);
    assert_eq!(view.yaw, 0.0, "from the front");
    assert!(view.pitch > 0.0 && view.pitch < 45.0, "a little above: {}", view.pitch);
    assert_eq!(view.target, [0.0, 0.5, 0.0], "the middle of the model");

    // A box of nothing, as a cloud's headers give, still gives a view that can be drawn from.
    let nothing = ModelFacts { kind: ModelKind::Points, triangles: 0, points: 10, bounds: ([0.0; 3], [0.0; 3]) };
    for facts in [nothing, ModelFacts { bounds: ([f32::NAN; 3], [f32::INFINITY; 3]), ..nothing }] {
        let view = View::framing(&facts);
        assert!(view.distance.is_finite() && view.distance > 0.0, "{view}");
        assert!(view.target.iter().all(|v| v.is_finite()), "{view}");
        assert!(View::parse(&view.to_string()).is_some());
    }
    // A cloud is framed by the box its points give once read.
    let cloud = model_of_cloud(500);
    let read_view = View::framing(&cloud.facts);
    assert!(read_view.distance > 2.0, "the sphere of radius 1 is seen whole: {read_view}");

    let Some(drawer) = drawer("framing_shows_the_whole_model") else { return };
    for (model, view) in [(model, view), (cloud, read_view)] {
        let scene = drawer.upload(model);
        for (width, height) in [(320, 180), (90, 160), (100, 100)] {
            let picture = picture(drawer.draw(&scene, view, width, height)).expect("drawn");
            let (l, t, r, b) = covered(&picture).expect("something is seen");
            assert!(l > 0 && t > 0 && r < width && b < height, "{width} by {height}: the model reaches the edge {:?}", (l, t, r, b));
            let smaller = width.min(height);
            assert!((r - l).max(b - t) * 3 > smaller, "{width} by {height}: the model is lost in the middle {:?}", (l, t, r, b));
            for x in 0..width {
                for y in [0, height - 1] {
                    assert_eq!(alpha(&picture, x, y), 0);
                }
            }
        }
    }
}

fn model_of_cloud(points: u32) -> Model {
    model(make_test_ply(points, true))
}

#[test]
fn between_starts_and_ends_at_its_views() {
    let a = View { yaw: 350.0, pitch: 10.0, distance: 2.0, target: [0.0, 0.0, 0.0] };
    let b = View { yaw: 10.0, pitch: 30.0, distance: 8.0, target: [1.0, 2.0, -3.0] };
    assert_eq!(View::between(a, b, 0.0), a);
    assert_eq!(View::between(a, b, 1.0), b);
    assert_eq!(View::between(a, b, -1.0), a);
    assert_eq!(View::between(a, b, 2.0), b);
    // Halfway it turns the short way round, through 0 rather than back through 180.
    let middle = View::between(a, b, 0.5);
    assert!(middle.yaw.rem_euclid(360.0) < 0.01 || middle.yaw.rem_euclid(360.0) > 359.99, "{middle}");
    assert!((middle.pitch - 20.0).abs() < 0.01, "{middle}");
    assert!(middle.distance > 2.0 && middle.distance < 8.0, "{middle}");
    assert!((middle.target[1] - 1.0).abs() < 0.01, "{middle}");
    // Smooth: slow at each end, and always on the way.
    let mut last = a;
    for step in 1..=100 {
        let now = View::between(a, b, step as f32 / 100.0);
        assert!(now.pitch >= last.pitch && now.distance >= last.distance, "{last} then {now}");
        last = now;
    }
    let early = View::between(a, b, 0.01);
    assert!((early.pitch - a.pitch) < 0.01 * (b.pitch - a.pitch), "it starts gently: {early}");
}

#[test]
fn a_textured_grid_shows_its_texture() {
    let Some(drawer) = drawer("a_textured_grid_shows_its_texture") else { return };
    for jpeg in [false, true] {
        let scene = drawer.upload(model(make_test_glb(MadeGlb::Grid { cells: 4, texture: 64, jpeg })));
        let view = View { yaw: 0.0, pitch: 0.0, distance: 4.0, target: [0.0, 0.0, 0.0] };
        let picture = picture(drawer.draw(&scene, view, 200, 200)).expect("drawn");
        for (quarter, (x, y)) in [(50, 50), (150, 50), (50, 150), (150, 150)].into_iter().enumerate() {
            let got = rgba(&picture, x, y);
            let want = GRID_COLOURS[quarter];
            for channel in 0..3 {
                assert!(got[channel].abs_diff(want[channel]) <= 32, "jpeg {jpeg}: the quarter {quarter} is {got:?}, its texture {want:?}");
            }
            assert_eq!(got[3], 255);
        }
    }
}

#[test]
fn a_cloud_is_drawn_as_points() {
    let Some(drawer) = drawer("a_cloud_is_drawn_as_points") else { return };
    let cloud = model_of_cloud(2000);
    let view = View::framing(&cloud.facts);
    let scene = drawer.upload(cloud);
    let picture = picture(drawer.draw(&scene, view, 200, 200)).expect("drawn");
    let seen = picture.bgra.chunks(4).filter(|p| p[3] > 0).count();
    let (l, t, r, b) = covered(&picture).expect("points are seen");
    let area = ((r - l) * (b - t)) as usize;
    assert!(seen >= 300, "only {seen} pixels show points");
    assert!(seen * 2 < area, "{seen} of {area} pixels are covered: a surface, not points");
    // Each point is in its own colour, not a colour of the drawer's.
    let coloured = picture.bgra.chunks(4).filter(|p| p[3] > 127 && (p[0].abs_diff(p[1]) > 20 || p[1].abs_diff(p[2]) > 20)).count();
    assert!(coloured > 0, "the points are all grey");
}

/// Compares `picture` with `tests/golden/<name>.png` within `tolerance` a channel, for all but a
/// hundredth of its pixels. `UPDATE_GOLDEN=1` rewrites the file; a mismatch writes
/// `target/golden-actual/<name>.png` and panics with its path.
fn assert_close_to_golden(name: &str, picture: &Picture, tolerance: u8) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let golden = root.join("tests/golden").join(format!("{name}.png"));
    let rgba: Vec<u8> = picture.bgra.chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    let encode = |rgba: &[u8]| {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, picture.width, picture.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(rgba).unwrap();
        writer.finish().unwrap();
        out
    };
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, encode(&rgba)).unwrap();
        return;
    }
    let stored = std::fs::read(&golden).unwrap_or_else(|_| panic!("missing golden {}; run with UPDATE_GOLDEN=1", golden.display()));
    let mut reader = png::Decoder::new(Cursor::new(stored)).read_info().unwrap();
    let mut expected = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut expected).unwrap();
    let off = if (info.width, info.height) != (picture.width, picture.height) {
        usize::MAX
    } else {
        rgba.chunks(4).zip(expected.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(a, b)| a.abs_diff(*b) > tolerance)).count()
    };
    if off > rgba.len() / 4 / 100 {
        let out = root.join("../../target/golden-actual");
        std::fs::create_dir_all(&out).unwrap();
        let out = out.join(format!("{name}.png"));
        std::fs::write(&out, encode(&rgba)).unwrap();
        panic!("golden {name} differs at {off} pixels; actual written to {}", out.display());
    }
}

#[test]
fn a_still_is_close_to_the_stored_one_on_the_software_adapter() {
    let drawer = start(DrawerOptions { only: Some(AdapterKind::Software), ..DrawerOptions::default() });
    if let Err(why) = wait_ready(&drawer) {
        eprintln!("a_still_is_close_to_the_stored_one_on_the_software_adapter skipped: no software adapter here. {why}");
        return;
    }
    let model = boxes();
    let view = View { yaw: 35.0, pitch: 25.0, ..View::framing(&model.facts) };
    let scene = drawer.upload(model);
    let picture = picture(drawer.draw(&scene, view, 160, 90)).expect("drawn");
    assert_close_to_golden("box-still", &picture, 12);
}

#[test]
fn with_no_adapter_a_model_is_a_panel_and_says_why() {
    let drawer = start(DrawerOptions { no_adapter: true, ..DrawerOptions::default() });
    assert_eq!(wait_ready(&drawer), Err(CantDraw::NoAdapter));
    let scene = drawer.upload(boxes());
    let view = View::framing(&boxes().facts);
    for _ in 0..2 {
        assert_eq!(picture(drawer.draw(&scene, view, 64, 36)).unwrap_err(), NotDrawn::Cant(CantDraw::NoAdapter));
    }
    assert_eq!(
        CantDraw::NoAdapter.to_string(),
        "This computer can't draw 3D models here. The slide shows a still when it's presented somewhere that can."
    );
}

#[test]
fn a_lost_device_is_made_again() {
    let quick = DrawerOptions { retry_after: Duration::from_millis(50), ..DrawerOptions::default() };
    let Some(drawer) = drawer_with("a_lost_device_is_made_again", quick) else { return };
    let model = || boxes();
    let view = View::framing(&model().facts);
    let mut scene = drawer.upload(model());
    let before = picture(drawer.draw(&scene, view, 120, 90)).expect("drawn");
    let kind = drawer.ready();
    assert_eq!(drawer.devices_made(), 1);
    for again in 1..=2 {
        drawer.lose_device();
        // The model went with the device: it's to be uploaded again, to the device made anew.
        assert_eq!(picture(drawer.draw(&scene, view, 120, 90)).unwrap_err(), NotDrawn::UploadAgain);
        assert_eq!(drawer.devices_made(), 1 + again);
        assert_eq!(drawer.ready(), kind);
        scene = drawer.upload(model());
        let after = picture(drawer.draw(&scene, view, 120, 90)).expect("drawn again on a new device");
        assert_eq!(covered(&after), covered(&before));
    }

    // Making it again fails: nothing can be drawn until a later try makes one.
    drawer.fail_making(2);
    drawer.lose_device();
    assert_eq!(picture(drawer.draw(&scene, view, 120, 90)).unwrap_err(), NotDrawn::Cant(CantDraw::NoAdapter));
    assert_eq!(drawer.ready(), Some(Err(CantDraw::NoAdapter)));
    // Asked again at once, it waits before trying again.
    assert_eq!(picture(drawer.draw(&scene, view, 120, 90)).unwrap_err(), NotDrawn::Cant(CantDraw::NoAdapter));
    // The second try fails too, and the wait before the next doubles.
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(picture(drawer.draw(&scene, view, 120, 90)).unwrap_err(), NotDrawn::Cant(CantDraw::NoAdapter));
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(picture(drawer.draw(&scene, view, 120, 90)).unwrap_err(), NotDrawn::Cant(CantDraw::NoAdapter), "the wait doubled");
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(picture(drawer.draw(&scene, view, 120, 90)).unwrap_err(), NotDrawn::UploadAgain);
    assert_eq!(drawer.devices_made(), 4);
    assert_eq!(drawer.ready(), kind);
    let scene = drawer.upload(model());
    let after = picture(drawer.draw(&scene, view, 120, 90)).expect("drawn once a device could be made");
    assert_eq!(covered(&after), covered(&before));
}

#[test]
fn what_is_too_large_for_the_device_says_so_and_keeps_the_device() {
    let Some(drawer) = drawer("what_is_too_large_for_the_device_says_so_and_keeps_the_device") else { return };
    let model = boxes();
    let view = View::framing(&model.facts);
    let scene = drawer.upload(model);
    for (width, height) in [(8192, 8192), (100_000, 10)] {
        assert_eq!(picture(drawer.draw(&scene, view, width, height)).unwrap_err(), NotDrawn::Cant(CantDraw::TooLarge));
    }
    assert!(picture(drawer.draw(&scene, view, 64, 36)).is_ok());
    assert_eq!(drawer.devices_made(), 1, "a picture too large isn't a lost device");

    // A device whose buffers can't hold even one triangle: the model is too large, and stays so.
    let Some(tiny) = drawer_with("what_is_too_large_for_the_device_says_so_and_keeps_the_device", DrawerOptions { most_buffer: Some(8), ..DrawerOptions::default() }) else { return };
    let scene = tiny.upload(boxes());
    for _ in 0..2 {
        assert_eq!(picture(tiny.draw(&scene, view, 64, 36)).unwrap_err(), NotDrawn::Cant(CantDraw::TooLarge));
    }
    assert_eq!(tiny.devices_made(), 1);
    assert_eq!(tiny.ready().map(|r| r.is_ok()), Some(true));
}

#[test]
fn a_model_too_large_for_one_buffer_is_held_in_several() {
    let Some(whole) = drawer("a_model_too_large_for_one_buffer_is_held_in_several") else { return };
    let small = DrawerOptions { most_buffer: Some(4096), ..DrawerOptions::default() };
    let Some(split) = drawer_with("a_model_too_large_for_one_buffer_is_held_in_several", small) else { return };
    // A grid of 441 corners and 800 triangles, and a cloud of 2,000 points, each many times 4 KB.
    for model in [model(make_test_glb(MadeGlb::Grid { cells: 20, texture: 64, jpeg: false })), model_of_cloud(2000)] {
        let view = View::framing(&model.facts);
        let a = picture(whole.draw(&whole.upload(model.clone()), view, 160, 120)).expect("drawn");
        let b = picture(split.draw(&split.upload(model), view, 160, 120)).expect("drawn in pieces");
        assert_eq!(covered(&a), covered(&b));
        let differ = a.bgra.iter().zip(&b.bgra).filter(|(x, y)| x.abs_diff(**y) > 2).count();
        assert!(differ < a.bgra.len() / 100, "{differ} bytes differ");
    }
}

#[test]
fn a_model_of_one_point_or_far_out_is_drawn() {
    let Some(drawer) = drawer("a_model_of_one_point_or_far_out_is_drawn") else { return };
    for at in [[5.0, 5.0, 5.0], [1e30, -1e30, 1e30]] {
        let facts = ModelFacts { kind: ModelKind::Points, triangles: 0, points: 1, bounds: (at, at) };
        let model = Model {
            meshes: Vec::new(),
            points: Some(Points { positions: vec![at], colours: vec![[255, 0, 0, 255]] }),
            texture: None,
            facts,
        };
        let view = View::framing(&facts);
        assert!(View::parse(&view.to_string()).is_some(), "{view}");
        let scene = drawer.upload(model);
        let picture = picture(drawer.draw(&scene, view, 64, 64)).expect("drawn");
        if at[0] == 5.0 {
            assert!((30..34).any(|x| (30..34).any(|y| alpha(&picture, x, y) > 0)), "the point is in the middle");
        }
    }
    // Bounds as wide as numbers go still frame a model with a distance that can be written.
    let wide = ModelFacts { kind: ModelKind::Mesh, triangles: 1, points: 0, bounds: ([-3e38; 3], [3e38; 3]) };
    let view = View::framing(&wide);
    assert!(view.distance.is_finite() && view.distance > 0.0, "{view}");
    assert_eq!(view.target, [0.0; 3]);
}

#[test]
fn only_the_newest_picture_of_a_scene_is_drawn() {
    let slow = DrawerOptions { device_delay: Duration::from_millis(300), ..DrawerOptions::default() };
    let Some(drawer) = drawer_with("only_the_newest_picture_of_a_scene_is_drawn", slow) else { return };
    let model = boxes();
    let view = View::framing(&model.facts);
    let scene = drawer.upload(model);
    let other = drawer.upload(boxes());
    let third = drawer.upload(boxes());
    // Asked for while the device is being made, these wait together.
    let busy = drawer.draw(&third, view, 1600, 900);
    let first = drawer.draw(&scene, view, 64, 36);
    let dropped = drawer.draw(&other, view, 64, 36);
    drop(dropped);
    let second = drawer.draw(&scene, View { yaw: 10.0, ..view }, 64, 36);
    let elsewhere = drawer.draw(&other, view, 64, 36);
    assert!(picture(busy).is_ok());
    assert_eq!(picture(first).unwrap_err(), NotDrawn::Replaced);
    assert!(picture(second).is_ok());
    assert!(picture(elsewhere).is_ok(), "another scene's picture isn't replaced");
}

#[test]
fn a_closed_drawer_says_so() {
    let drawer = Drawer::start_with(DrawerOptions { only: Some(AdapterKind::Software), ..DrawerOptions::default() });
    let ready = wait_ready(&drawer);
    let scene = drawer.upload(boxes());
    assert!(drawer.clone().close(Duration::from_secs(30)));
    assert_eq!(drawer.ready(), Some(Err(CantDraw::NoAdapter)), "was {ready:?}");
    assert_eq!(picture(drawer.draw(&scene, View::framing(&boxes().facts), 32, 18)).unwrap_err(), NotDrawn::Closed);
}

#[test]
fn draw_never_blocks_the_caller() {
    // A drawer whose device takes a while: asking it to draw returns at once all the same.
    let slow = start(DrawerOptions { device_delay: Duration::from_millis(500), ..DrawerOptions::default() });
    assert_eq!(slow.ready(), None);
    let box_model = boxes();
    let view = View::framing(&box_model.facts);
    let started = Instant::now();
    let scene = slow.upload(box_model);
    let answer = slow.draw(&scene, view, 64, 36);
    assert!(started.elapsed() < Duration::from_millis(1), "took {:?}", started.elapsed());
    drop(answer);

    let Some(drawer) = drawer("draw_never_blocks_the_caller") else { return };
    let grid = model(make_test_glb(MadeGlb::Grid { cells: 200, texture: 256, jpeg: false }));
    let view = View::framing(&grid.facts);
    let scene = drawer.upload(grid);
    let first = drawer.draw(&scene, view, 1600, 900);
    let started = Instant::now();
    let second = drawer.draw(&scene, View { yaw: 20.0, ..view }, 1600, 900);
    assert!(started.elapsed() < Duration::from_millis(1), "took {:?} while a draw ran", started.elapsed());
    // Asked for at once, the first may be replaced by the second before it's begun.
    assert!(matches!(picture(first), Ok(_) | Err(NotDrawn::Replaced)));
    assert!(picture(second).is_ok());
}

#[test]
fn two_drawers_do_not_fight() {
    let Some(one) = drawer("two_drawers_do_not_fight") else { return };
    let Some(two) = drawer("two_drawers_do_not_fight") else { return };
    let pictures = std::thread::scope(|scope| {
        let work = |drawer: &Drawer| {
            let model = boxes();
            let view = View::framing(&model.facts);
            let scene = drawer.upload(model);
            (0..4)
                .map(|step| picture(drawer.draw(&scene, View { yaw: step as f32 * 20.0, ..view }, 160, 90)).expect("drawn"))
                .collect::<Vec<_>>()
        };
        let (one, two) = (&one, &two);
        let a = scope.spawn(move || work(one));
        let b = scope.spawn(move || work(two));
        (a.join().unwrap(), b.join().unwrap())
    });
    for (a, b) in pictures.0.iter().zip(&pictures.1) {
        assert_eq!(covered(a), covered(b));
        assert!(covered(a).is_some());
    }
    // A scene belongs to the drawer it was uploaded to.
    let scene = one.upload(boxes());
    assert_eq!(picture(two.draw(&scene, View::framing(&boxes().facts), 32, 18)).unwrap_err(), NotDrawn::UploadAgain);
}

/// The median and the 99th percentile of `times`.
fn spread(times: &mut [Duration]) -> (Duration, Duration) {
    times.sort();
    (times[times.len() / 2], times[(times.len() * 99 / 100).min(times.len() - 1)])
}

#[test]
#[ignore = "a timing run: cargo test --release -p slides-model --features make --test draw -- --ignored --nocapture"]
fn a_million_triangles_on_each_adapter() {
    // 708 by 708 squares of two triangles each is a little over a million triangles.
    let grid = model(make_test_glb(MadeGlb::Grid { cells: 708, texture: 4096, jpeg: false }));
    println!("{} triangles", grid.facts.triangles);
    for kind in adapter_kinds() {
        let drawer = start(DrawerOptions { only: Some(kind), ..DrawerOptions::default() });
        if wait_ready(&drawer).is_err() {
            println!("{kind:?}: no device could be made");
            continue;
        }
        let view = View::framing(&grid.facts);
        let scene = drawer.upload(grid.clone());
        let _warm = picture(drawer.draw(&scene, view, 1600, 900)).expect("drawn");
        let (mut drawing, mut reading) = (Vec::new(), Vec::new());
        for step in 0..60 {
            let picture = picture(drawer.draw(&scene, View { yaw: step as f32 * 3.0, ..view }, 1600, 900)).expect("drawn");
            drawing.push(picture.drawing);
            reading.push(picture.reading_back);
        }
        let (draw_median, draw_most) = spread(&mut drawing);
        let (read_median, read_most) = spread(&mut reading);
        println!(
            "{kind:?}, 1600 by 900: drawing {draw_median:?} median, {draw_most:?} at the 99th percentile; reading back {read_median:?} median, {read_most:?} at the 99th percentile"
        );
    }
}
