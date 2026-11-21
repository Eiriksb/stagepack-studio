# Live content: the two spikes

Two throwaway builds, on 2026-11-18, on a 16-core Linux laptop with an NVIDIA RTX PRO 500 and an Intel integrated graphics card, in release builds, outside the repo. Each answered one question from `docs/superpowers/specs/2026-11-18-live-content-design.md`. Both answers are **go, with conditions**. The code wasn't kept.

Neither could measure how smooth playback looks on a screen: the nested display they had to use shows about 8 frames a second even for an empty window. That is a hand check on a real Linux display and on Windows, in each plan's record.

## 1. Video

**Question:** can an H.264 MP4 be decoded and shown on a slide smoothly, with a very large file read from disk and memory staying flat?

Three of the owner's own clips were used, read in place and not kept: two short loops and one of 392 MB. All are H.264 High profile, 30 frames a second, with B-frames.

With `openh264` 0.9.10 (Cisco's library, built from source), `mp4` 0.14.0 to read the container and `yuvutils-rs` 0.8.3 to turn frames into the colours the window wants, on one thread:

| Clip | Frames decoded | Decode, median | Decode, 99th percentile |
| --- | --- | --- | --- |
| 960 by 486 | 444 of 444 | 1.0 ms | 3.3 ms |
| 1120 by 630 | 308 of 308 | 4.0 ms | 10 ms |
| 1920 by 1080 | 7,065 of 7,065 | 14.5 ms | 70 ms |

- **1080p at 30 frames a second is the ceiling.** Its average decode is 22.6 ms against a 33 ms frame, but 28% of frames take longer, so playback must decode ahead into a short queue.
- **Turning a frame into window colours** costs 0.06 to 0.85 ms.
- **Showing:** frames go through `paint_image`, releasing each replaced one with `drop_image`. That costs about 1.5 to 1.9 ms a 1080p frame. With the release, memory is flat: 245 MB for one clip, 295 MB for two, 348 MB at 1080p, and graphics memory flat. Without it, graphics memory grows 38 MB a second. `paint_surface` is for macOS only.
- **The large file** is read in pieces: one pass reads each byte once. The first frame is decoded 108 ms after opening. Looping back to the start takes 12 ms. Memory after two minutes is 348 MB, flat.
- **CPU:** 1.1 cores for one clip, 1.3 for two, 1.4 at 1080p.

Conditions, for the plan:

- Tell the decoder not to flush after each frame, and flush at the end. Its default drops most frames of a video with B-frames (9 of 444 came out).
- One decoding thread a clip. The decoder's own threading crashes.
- A queue of 10 to 15 decoded frames, timed from the file's own timestamps.
- Read a plain file, not a buffered one: buffering re-read 89 MB for a 0.5 MB clip.
- Always release a replaced frame.
- Take the colour matrix from the file.
- Only 8-bit 4:2:0 H.264 in an `avc1` MP4 plays. Anything else shows its still and says so.
- Not tried: a variable frame rate, and a file whose index is at its end.
- If 4K, 60 frames a second or 10-bit video matters later, the next thing to try is each system's own decoder.

## 2. 3D

**Question:** can a model from a file be drawn on a slide, turning smoothly, and can a still be drawn with no usable graphics card?

A made-up terrain of a million triangles with a 4,096-pixel texture (a 58 MB `.glb`), a model of several small meshes, and a point cloud of five million points, with `gltf` 1.4.1 and `wgpu` 30.0.1, the version the window toolkit already builds.

| What | NVIDIA, 1600 by 900 | NVIDIA, 3200 by 1800 | Intel, 1600 by 900 |
| --- | --- | --- | --- |
| Draw the mesh | 0.63 ms | 1.0 ms | 3.5 ms |
| Read the picture back | 1.8 ms | 6.6 ms | 61 to 108 ms (17 ms copied on 8 threads) |
| Draw the point cloud | 2.15 ms | 2.4 ms | 8.8 ms |

- **Loading:** the 58 MB model parses in 190 to 435 ms and peaks at 231 MB, most of it the texture. The point cloud parses in 42 to 77 ms.
- **In a window:** the picture is read back from the graphics card and shown as a layer through `paint_image`, as video frames are. Memory is flat over a minute: about 300 MB, and 291 to 299 MB on the card. The window toolkit has no way to show a picture straight from the card on Linux or Windows.
- **No graphics card:** with Mesa's software renderer, which this computer has, a still of the million-triangle model takes 0.88 s, and it can turn at about 18 frames a second. A Linux computer without it has no software path. Windows has one built in, not measured.
- **The Intel card's read-back is the main risk**, as many laptops have only that. Copying on several threads, or drawing a smaller picture, brings it within a frame.

Conditions, for the plan:

- Make the 3D device on a worker thread, the first time a model is shown. It took about a second, and must never hold up drawing the window.
- Keep a still of each model in the talk, from when it was last drawn, for computers that can't draw it and for thumbnails and exports.
- On an integrated card, copy the read-back on several threads or draw smaller.
- Limits on a model, checked before anything is decoded: 256 MB, everything inside the one file, 4 million triangles, a texture 8,192 pixels a side at most, 64 million texture pixels in all.
- Check the frame rate by hand on a real Linux display and on Windows, where the window toolkit draws through DirectX and the 3D would have a device of its own.
