// Light as the picture shows it, for the shaders that draw their own colour (the braziers' fire,
// the effects). With HDR (the WebGPU build's high tier) the camera tonemaps in a pass of its own
// and this does nothing. Without it (WebGL2, the low tier) Bevy's materials tonemap in their
// shaders, and so must these: a fire or a blast brighter than white otherwise clips to a flat
// white shape (the art suite's pictures: white cones on the braziers, a white disc for a blast).
#define_import_path el::display

#import bevy_pbr::mesh_view_bindings::view
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

fn display(rgb: vec3<f32>) -> vec3<f32> {
#ifdef TONEMAP_IN_SHADER
    return tone_mapping(vec4(rgb, 1.0), view.color_grading).rgb;
#else
    return rgb;
#endif
}
