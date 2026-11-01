// One pass of the effect chain. Each effect is a case in `effect`, numbered as in PictureKind::ALL.

struct Uniforms {
    kind: u32,
    time: f32,
    aspect: f32,
    area_on: u32,
    area: vec4<f32>,
    // feather, ellipse, invert, unused
    area_style: vec4<f32>,
    p: array<vec4<f32>, 3>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var input: texture_2d<f32>;
@group(0) @binding(2) var input_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    // One triangle that covers the whole target.
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    var out: VertexOut;
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

const PI: f32 = 3.14159265;

fn param(i: u32) -> f32 {
    return u.p[i / 4u][i % 4u];
}

fn sample(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(input, input_sampler, uv, 0.0);
}

// Black outside the picture, rather than stretched edge pixels.
fn sample_bounded(uv: vec2<f32>) -> vec4<f32> {
    let inside = step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);
    return vec4<f32>(sample(uv).rgb * inside, 1.0);
}

fn rotate(p: vec2<f32>, degrees: f32) -> vec2<f32> {
    let a = degrees * PI / 180.0;
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
}

fn hash(x: f32) -> f32 {
    return fract(sin(x * 127.1) * 43758.5453);
}

// Moves a point about the picture's centre, keeping pixels square.
fn around_centre(uv: vec2<f32>, offset: vec2<f32>, zoom: f32, degrees: f32) -> vec2<f32> {
    var p = uv - vec2<f32>(0.5) - offset;
    p.x *= u.aspect;
    p = rotate(p, -degrees) / max(zoom, 0.001);
    p.x /= u.aspect;
    return p + vec2<f32>(0.5);
}

fn hue_rotate(c: vec3<f32>, degrees: f32) -> vec3<f32> {
    let a = degrees * PI / 180.0;
    let k = vec3<f32>(0.57735);
    return c * cos(a) + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - cos(a));
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    switch u.kind {
        case 1u: { // Transform
            let moved = around_centre(uv, vec2<f32>(param(0u), -param(1u)) * 0.5, param(2u), param(3u));
            return vec4<f32>(sample_bounded(moved).rgb * param(4u), 1.0);
        }
        case 2u: { // Color
            var c = sample(uv).rgb + vec3<f32>(param(0u));
            c = (c - vec3<f32>(0.5)) * param(1u) + vec3<f32>(0.5);
            let grey = vec3<f32>(dot(c, vec3<f32>(0.299, 0.587, 0.114)));
            c = mix(grey, c, param(2u));
            c = hue_rotate(c, param(3u));
            c = mix(c, vec3<f32>(1.0) - c, param(4u));
            return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
        }
        case 3u: { // Blur: a spiral of samples, which looks smooth with far fewer than a grid
            const TAPS: i32 = 40;
            let radius = param(0u) * 0.08;
            var sum = vec3<f32>(0.0);
            for (var i = 0; i < TAPS; i++) {
                let f = (f32(i) + 0.5) / f32(TAPS);
                let a = f32(i) * 2.39996;
                let offset = vec2<f32>(cos(a) / u.aspect, sin(a)) * sqrt(f) * radius;
                sum += sample(uv + offset).rgb;
            }
            return vec4<f32>(sum / f32(TAPS), 1.0);
        }
        case 4u: { // Warp
            let centre = vec2<f32>(param(0u), param(1u));
            var d = uv - centre;
            d.x *= u.aspect;
            let radius = max(param(2u), 0.001);
            let n = length(d) / radius;
            if n < 1.0 {
                let falloff = (1.0 - n) * (1.0 - n);
                // Positive bulge magnifies by sampling nearer the centre.
                d = d * (1.0 - clamp(param(3u), -1.0, 1.0) * falloff * 0.8);
                d = rotate(d, param(4u) * falloff);
            }
            d.x /= u.aspect;
            return sample_bounded(centre + d);
        }
        case 5u: { // RGB split
            let offset = rotate(vec2<f32>(param(0u), 0.0), param(1u));
            return vec4<f32>(sample(uv + offset).r, sample(uv).g, sample(uv - offset).b, 1.0);
        }
        case 6u: { // Pixelate
            let size = vec2<f32>(max(param(0u), 0.0005) / u.aspect, max(param(0u), 0.0005));
            let snapped = (floor(uv / size) + vec2<f32>(0.5)) * size;
            let levels = max(param(1u), 2.0) - 1.0;
            return vec4<f32>(floor(sample(snapped).rgb * levels + vec3<f32>(0.5)) / levels, 1.0);
        }
        case 7u: { // Glitch: rows of blocks jump sideways, and split their colours when they do
            let tick = floor(u.time * param(2u));
            let row = floor(uv.y * param(1u));
            let chosen = step(1.0 - param(0u), hash(row + tick * 13.0));
            let shift = (hash(row * 3.0 + tick) - 0.5) * 2.0 * param(3u) * chosen;
            let split = shift * 0.3;
            return vec4<f32>(sample(uv + vec2<f32>(shift + split, 0.0)).r, sample(uv + vec2<f32>(shift, 0.0)).g, sample(uv + vec2<f32>(shift - split, 0.0)).b, 1.0);
        }
        case 8u: { // Shake
            let t = u.time * param(1u);
            let offset = vec2<f32>(sin(t * 1.0) + sin(t * 2.3) * 0.5, cos(t * 1.3) + sin(t * 3.1) * 0.5) * param(0u) * 0.66;
            let turn = sin(t * 1.7) * param(2u);
            // Pushed in a little, so that shaking doesn't reveal the edges.
            return sample_bounded(around_centre(uv, offset, 1.0 + param(0u) * 2.0, turn));
        }
        case 9u: { // Mirror: folds the picture around its centre
            var p = uv - vec2<f32>(0.5);
            p.x *= u.aspect;
            p = rotate(p, param(1u));
            let segment = PI * 2.0 / max(floor(param(0u)) * 2.0, 2.0);
            var a = atan2(p.y, p.x);
            a = abs(((a % (segment * 2.0)) + segment * 2.0) % (segment * 2.0) - segment);
            p = vec2<f32>(cos(a), sin(a)) * length(p);
            p = rotate(p, -param(1u));
            p.x /= u.aspect;
            return sample_bounded(p + vec2<f32>(0.5));
        }
        case 10u: { // Flash
            let tint = hue_rotate(vec3<f32>(1.0, 0.0, 0.0), param(1u));
            let colour = mix(vec3<f32>(1.0), clamp(tint, vec3<f32>(0.0), vec3<f32>(1.0)), param(2u));
            return vec4<f32>(mix(sample(uv).rgb, colour, param(0u)), 1.0);
        }
        default: {
            return sample(uv);
        }
    }
}

// 1 inside the area, 0 outside, with a soft edge.
fn area_mask(uv: vec2<f32>) -> f32 {
    let half_size = max(u.area.zw * 0.5, vec2<f32>(0.001));
    let d = abs(uv - u.area.xy) / half_size;
    let edge = select(max(d.x, d.y), length(d), u.area_style.y > 0.5);
    let feather = max(u.area_style.x, 0.001);
    let inside = 1.0 - smoothstep(1.0 - feather, 1.0, edge);
    return select(inside, 1.0 - inside, u.area_style.z > 0.5);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let result = effect(in.uv);
    if u.area_on == 0u {
        return result;
    }
    return mix(sample(in.uv), result, area_mask(in.uv));
}
