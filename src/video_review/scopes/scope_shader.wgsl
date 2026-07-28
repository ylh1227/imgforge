// Fullscreen triangle + scope fragment shading (histogram / waveform / vectorscope).

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct Uniforms {
    kind: u32,
    mode: u32,
    scale: u32,
    show_box: u32,
    out_w: u32,
    out_h: u32,
    data_w: u32,
    data_h: u32,
    skin_angle: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var data_tex: texture_2d<f32>;
@group(0) @binding(2) var data_samp: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var out: VsOut;
    out.clip = vec4<f32>(positions[vi], 0.0, 1.0);
    out.uv = uvs[vi];
    return out;
}

fn sample_data(uv: vec2<f32>) -> f32 {
    return textureSample(data_tex, data_samp, uv).r;
}

fn draw_vline(uv: vec2<f32>, x: f32, thickness: f32) -> f32 {
    return select(0.0, 1.0, abs(uv.x - x) < thickness);
}

fn draw_hline(uv: vec2<f32>, y: f32, thickness: f32) -> f32 {
    return select(0.0, 1.0, abs(uv.y - y) < thickness);
}

fn hist_color(channel: u32) -> vec3<f32> {
    if channel == 0u {
        return vec3<f32>(0.85, 0.85, 0.88);
    }
    if channel == 1u {
        return vec3<f32>(0.95, 0.25, 0.22);
    }
    if channel == 2u {
        return vec3<f32>(0.25, 0.85, 0.35);
    }
    return vec3<f32>(0.30, 0.45, 0.98);
}

fn render_histogram(uv: vec2<f32>) -> vec4<f32> {
    var rgb = vec3<f32>(0.05, 0.05, 0.06);
    let levels = array<f32, 4>(0.0, 16.0 / 255.0, 235.0 / 255.0, 1.0);
    for (var i = 0u; i < 4u; i = i + 1u) {
        let g = draw_vline(uv, levels[i], 0.0025);
        rgb = mix(rgb, vec3<f32>(0.35, 0.35, 0.38), g * 0.6);
    }

    if u.mode == 0u {
        // Parade: four panels side by side (Y R G B).
        let panel = clamp(u32(uv.x * 4.0), 0u, 3u);
        let local_x = fract(uv.x * 4.0);
        let h = sample_data(vec2<f32>(local_x, (f32(panel) + 0.5) / 4.0));
        let bar = select(0.0, 1.0, (1.0 - uv.y) <= h && h > 0.001);
        rgb = mix(rgb, hist_color(panel), bar * 0.9);
    } else if u.mode == 1u {
        // Overlay
        for (var c = 0u; c < 4u; c = c + 1u) {
            let h = sample_data(vec2<f32>(uv.x, (f32(c) + 0.5) / 4.0));
            let bar = select(0.0, 1.0, (1.0 - uv.y) <= h && h > 0.001);
            rgb = max(rgb, hist_color(c) * bar);
        }
    } else {
        // Stack
        let panel = clamp(u32(uv.y * 4.0), 0u, 3u);
        let local_y = fract(uv.y * 4.0);
        let h = sample_data(vec2<f32>(uv.x, (f32(panel) + 0.5) / 4.0));
        let bar = select(0.0, 1.0, (1.0 - local_y) <= h && h > 0.001);
        rgb = mix(rgb, hist_color(panel), bar * 0.9);
        let sep = draw_hline(uv, f32(panel) * 0.25, 0.002);
        rgb = mix(rgb, vec3<f32>(0.2, 0.2, 0.22), sep);
    }
    return vec4<f32>(rgb, 1.0);
}

fn render_waveform(uv: vec2<f32>) -> vec4<f32> {
    var rgb = vec3<f32>(0.04, 0.05, 0.05);
    let ire = array<f32, 4>(0.0, 16.0 / 255.0, 235.0 / 255.0, 1.0);
    for (var i = 0u; i < 4u; i = i + 1u) {
        // top = white
        let y = 1.0 - ire[i];
        let g = draw_hline(uv, y, 0.002);
        rgb = mix(rgb, vec3<f32>(0.28, 0.32, 0.28), g * 0.7);
    }

    let intensity = sample_data(uv);
    if u.mode == 0u {
        let glow = vec3<f32>(0.25, 0.95, 0.45) * intensity;
        rgb = max(rgb, glow);
    } else {
        // RGB parade: three horizontal thirds of data already laid out in texture width.
        let third = u32(uv.x * 3.0);
        var tint = vec3<f32>(0.9, 0.25, 0.2);
        if third == 1u {
            tint = vec3<f32>(0.25, 0.9, 0.3);
        } else if third == 2u {
            tint = vec3<f32>(0.3, 0.45, 1.0);
        }
        rgb = max(rgb, tint * intensity);
        let sep = draw_vline(uv, 1.0 / 3.0, 0.002) + draw_vline(uv, 2.0 / 3.0, 0.002);
        rgb = mix(rgb, vec3<f32>(0.2, 0.2, 0.22), sep * 0.8);
    }
    return vec4<f32>(rgb, 1.0);
}

fn render_vectorscope(uv: vec2<f32>) -> vec4<f32> {
    let centered = uv * 2.0 - vec2<f32>(1.0, 1.0);
    let r = length(centered);
    var rgb = vec3<f32>(0.03, 0.03, 0.04);

    // Crosshair
    rgb = mix(rgb, vec3<f32>(0.25, 0.25, 0.28), draw_vline(uv, 0.5, 0.002));
    rgb = mix(rgb, vec3<f32>(0.25, 0.25, 0.28), draw_hline(uv, 0.5, 0.002));

    // Circle rings at ~75% / 100%
    let ring100 = abs(r - 1.0);
    let ring75 = abs(r - 0.75);
    if ring100 < 0.01 {
        rgb = mix(rgb, vec3<f32>(0.35, 0.35, 0.4), 0.8);
    }
    if u.show_box != 0u && ring75 < 0.01 {
        rgb = mix(rgb, vec3<f32>(0.45, 0.4, 0.2), 0.85);
    }

    // Skin tone line
    let ang = radians(u.skin_angle);
    let dir = vec2<f32>(cos(ang), -sin(ang));
    let proj = dot(centered, dir);
    let ortho = abs(centered.x * dir.y - centered.y * dir.x);
    if proj > 0.0 && proj < 0.95 && ortho < 0.012 {
        rgb = mix(rgb, vec3<f32>(0.85, 0.65, 0.35), 0.7);
    }

    // Primary / complementary tick marks (approx. angles)
    let ticks = array<f32, 6>(103.0, 241.0, 347.0, 23.0, 167.0, 283.0);
    let cols = array<vec3<f32>, 6>(
        vec3<f32>(0.9, 0.2, 0.2),
        vec3<f32>(0.2, 0.85, 0.3),
        vec3<f32>(0.25, 0.4, 1.0),
        vec3<f32>(0.2, 0.85, 0.85),
        vec3<f32>(0.9, 0.2, 0.85),
        vec3<f32>(0.95, 0.9, 0.2),
    );
    for (var i = 0u; i < 6u; i = i + 1u) {
        let a = radians(ticks[i]);
        let p = vec2<f32>(cos(a), -sin(a)) * 0.78;
        let d = distance(centered, p);
        if d < 0.03 {
            rgb = mix(rgb, cols[i], 0.9);
        }
    }

    let intensity = sample_data(uv);
    // Colorize scatter by angle for a more “broadcast” look.
    let hue = atan2(-centered.y, centered.x);
    let chroma = vec3<f32>(
        0.5 + 0.5 * cos(hue),
        0.5 + 0.5 * cos(hue - 2.094),
        0.5 + 0.5 * cos(hue + 2.094),
    );
    rgb = max(rgb, chroma * intensity);

    // Mask outside unit circle lightly
    if r > 1.02 {
        rgb = rgb * 0.35;
    }
    return vec4<f32>(rgb, 1.0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    if u.kind == 0u {
        return render_histogram(uv);
    }
    if u.kind == 1u {
        return render_waveform(uv);
    }
    return render_vectorscope(uv);
}
