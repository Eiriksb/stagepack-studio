// A mesh, its colours times its texture's, lit by one light from beside the eye and a little of
// every way.

struct Frame {
    view_projection: mat4x4<f32>,
    // Where the light comes from, of length one, in the model's space.
    light: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var picture: texture_2d<f32>;
@group(1) @binding(1) var picture_sampler: sampler;

struct Corner {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) coords: vec2<f32>,
    @location(3) colour: vec4<f32>,
}

struct Shaded {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) coords: vec2<f32>,
    @location(2) colour: vec4<f32>,
}

@vertex
fn vertex(corner: Corner) -> Shaded {
    var out: Shaded;
    out.clip = frame.view_projection * vec4<f32>(corner.position, 1.0);
    out.normal = corner.normal;
    out.coords = corner.coords;
    out.colour = corner.colour;
    return out;
}

@fragment
fn fragment(shaded: Shaded, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var normal = shaded.normal;
    let size = max(sqrt(dot(normal, normal)), 1e-20);
    normal = normal / size;
    // A face seen from behind is lit as its front would be.
    if !front {
        normal = -normal;
    }
    let lit = 0.35 + 0.65 * max(dot(normal, frame.light.xyz), 0.0);
    let texel = textureSample(picture, picture_sampler, shaded.coords);
    return vec4<f32>(shaded.colour.rgb * texel.rgb * lit, 1.0);
}
