// A cloud's points, each its own colour, unlit.

struct Frame {
    view_projection: mat4x4<f32>,
    light: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;

struct Point {
    @location(0) position: vec3<f32>,
    // sRGB, as the file gives it.
    @location(1) colour: vec4<f32>,
}

struct Shaded {
    @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec3<f32>,
}

fn linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@vertex
fn vertex(point: Point) -> Shaded {
    var out: Shaded;
    out.clip = frame.view_projection * vec4<f32>(point.position, 1.0);
    out.colour = vec3<f32>(linear(point.colour.r), linear(point.colour.g), linear(point.colour.b));
    return out;
}

@fragment
fn fragment(shaded: Shaded) -> @location(0) vec4<f32> {
    return vec4<f32>(shaded.colour, 1.0);
}
