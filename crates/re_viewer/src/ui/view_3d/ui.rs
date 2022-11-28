use egui::NumExt as _;
use glam::Affine3A;
use macaw::{vec3, Quat, Ray3, Vec3};
use smallvec::smallvec;

use re_data_store::{InstanceId, InstanceIdHash};
use re_log_types::{ObjPath, ViewCoordinates};
use re_renderer::{
    renderer::{GenericSkyboxDrawable, LineDrawable, MeshDrawable, PointCloudDrawable},
    view_builder::{TargetConfiguration, ViewBuilder},
    RenderContext,
};

use crate::{
    misc::{HoveredSpace, Selection},
    ViewerContext,
};

use super::{Eye, LineStrip, OrbitEye, Point3D, Scene3D, Size, SpaceCamera};

// ---

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub(crate) struct View3DState {
    orbit_eye: Option<OrbitEye>,

    #[serde(skip)]
    eye_interpolation: Option<EyeInterpolation>,

    /// What the mouse is hovering (from previous frame)
    #[serde(skip)]
    hovered_instance: Option<InstanceId>,

    /// Where in world space the mouse is hovering (from previous frame)
    #[serde(skip)]
    hovered_point: Option<glam::Vec3>,

    /// Estimate of the the bounding box of all data. Accumulated.
    #[serde(skip)]
    scene_bbox: macaw::BoundingBox,

    // options:
    spin: bool,
    show_axes: bool,

    #[serde(skip)]
    last_eye_interact_time: f64,

    /// Filled in at the start of each frame
    #[serde(skip)]
    pub(crate) space_specs: SpaceSpecs,
    #[serde(skip)]
    space_camera: Vec<SpaceCamera>,
}

impl Default for View3DState {
    fn default() -> Self {
        Self {
            orbit_eye: Default::default(),
            eye_interpolation: Default::default(),
            hovered_instance: Default::default(),
            hovered_point: Default::default(),
            scene_bbox: macaw::BoundingBox::nothing(),
            spin: false,
            show_axes: false,
            last_eye_interact_time: f64::NEG_INFINITY,
            space_specs: Default::default(),
            space_camera: Default::default(),
        }
    }
}

impl View3DState {
    fn update_eye(
        &mut self,
        ctx: &mut ViewerContext<'_>,
        tracking_camera: Option<Eye>,
        response: &egui::Response,
    ) -> &mut OrbitEye {
        if response.double_clicked() {
            // Reset eye
            if tracking_camera.is_some() {
                ctx.rec_cfg.selection = Selection::None;
            }
            self.interpolate_to_orbit_eye(default_eye(&self.scene_bbox, &self.space_specs));
        }

        if let Some(tracking_camera) = tracking_camera {
            if let Some(cam_interpolation) = &mut self.eye_interpolation {
                // Update interpolation target:
                cam_interpolation.target_orbit = None;
                if cam_interpolation.target_eye != Some(tracking_camera) {
                    cam_interpolation.target_eye = Some(tracking_camera);
                    response.ctx.request_repaint();
                }
            } else {
                self.interpolate_to_eye(tracking_camera);
            }
        }

        let orbit_camera = self
            .orbit_eye
            .get_or_insert_with(|| default_eye(&self.scene_bbox, &self.space_specs));

        if self.spin {
            orbit_camera.rotate(egui::vec2(
                -response.ctx.input().stable_dt.at_most(0.1) * 150.0,
                0.0,
            ));
            response.ctx.request_repaint();
        }

        if let Some(cam_interpolation) = &mut self.eye_interpolation {
            cam_interpolation.elapsed_time += response.ctx.input().stable_dt.at_most(0.1);

            let t = cam_interpolation.elapsed_time / cam_interpolation.target_time;
            let t = t.clamp(0.0, 1.0);
            let t = crate::math::ease_out(t);

            if t < 1.0 {
                response.ctx.request_repaint();
            }

            if let Some(target_orbit) = &cam_interpolation.target_orbit {
                *orbit_camera = cam_interpolation.start.lerp(target_orbit, t);
            } else if let Some(target_camera) = &cam_interpolation.target_eye {
                let camera = cam_interpolation.start.to_eye().lerp(target_camera, t);
                orbit_camera.copy_from_eye(&camera);
            } else {
                self.eye_interpolation = None;
            }
        }

        orbit_camera
    }

    fn interpolate_to_eye(&mut self, target: Eye) {
        if let Some(start) = self.orbit_eye {
            let target_time = EyeInterpolation::target_time(&start.to_eye(), &target);
            self.eye_interpolation = Some(EyeInterpolation {
                elapsed_time: 0.0,
                target_time,
                start,
                target_orbit: None,
                target_eye: Some(target),
            });
        } else {
            // shouldn't really happen (`self.orbit_eye` is only `None` for the first frame).
        }
    }

    fn interpolate_to_orbit_eye(&mut self, target: OrbitEye) {
        if let Some(start) = self.orbit_eye {
            let target_time = EyeInterpolation::target_time(&start.to_eye(), &target.to_eye());
            self.eye_interpolation = Some(EyeInterpolation {
                elapsed_time: 0.0,
                target_time,
                start,
                target_orbit: Some(target),
                target_eye: None,
            });
        } else {
            self.orbit_eye = Some(target);
        }
    }
}

#[derive(Clone)]
struct EyeInterpolation {
    elapsed_time: f32,
    target_time: f32,
    start: OrbitEye,
    target_orbit: Option<OrbitEye>,
    target_eye: Option<Eye>,
}

impl EyeInterpolation {
    pub fn target_time(start: &Eye, stop: &Eye) -> f32 {
        // Take more time if the rotation is big:
        let angle_difference = start
            .world_from_view
            .rotation()
            .angle_between(stop.world_from_view.rotation());

        egui::remap_clamp(angle_difference, 0.0..=std::f32::consts::PI, 0.2..=0.7)
    }
}

pub(crate) fn show_settings_ui(
    ctx: &mut ViewerContext<'_>,
    ui: &mut egui::Ui,
    state: &mut View3DState,
) {
    {
        let up_response = if let Some(up) = state.space_specs.up {
            if up == Vec3::X {
                ui.label("Up: +X")
            } else if up == -Vec3::X {
                ui.label("Up: -X")
            } else if up == Vec3::Y {
                ui.label("Up: +Y")
            } else if up == -Vec3::Y {
                ui.label("Up: -Y")
            } else if up == Vec3::Z {
                ui.label("Up: +Z")
            } else if up == -Vec3::Z {
                ui.label("Up: -Z")
            } else if up != Vec3::ZERO {
                ui.label(format!("Up: [{:.3} {:.3} {:.3}]", up.x, up.y, up.z))
            } else {
                ui.label("Up: —")
            }
        } else {
            ui.label("Up: —")
        };

        up_response.on_hover_ui(|ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label("Set with ");
                ui.code("rerun.log_view_coordinates");
                ui.label(".");
            });
        });
    }

    if ui
        .button("Reset virtual camera")
        .on_hover_text(
            "Resets camera position & orientation.\nYou can also double-click the 3D view",
        )
        .clicked()
    {
        state.orbit_eye = Some(default_eye(&state.scene_bbox, &state.space_specs));
        state.eye_interpolation = None;
        // TODO(emilk): reset tracking camera too
    }

    ui.checkbox(&mut state.spin, "Spin virtual camera")
        .on_hover_text("Spin view");
    ui.checkbox(&mut state.show_axes, "Show origin axes")
        .on_hover_text("Show X-Y-Z axes");

    if !state.space_camera.is_empty() {
        ui.checkbox(
            &mut ctx.options.show_camera_mesh_in_3d,
            "Show camera meshes",
        );
    }
}

#[derive(Clone, Default)]
pub(crate) struct SpaceSpecs {
    up: Option<glam::Vec3>,
    right: Option<glam::Vec3>,
}

impl SpaceSpecs {
    pub fn from_view_coordinates(coordinates: Option<ViewCoordinates>) -> Self {
        let up = (|| Some(coordinates?.up()?.as_vec3().into()))();
        let right = (|| Some(coordinates?.right()?.as_vec3().into()))();

        Self { up, right }
    }
}

/// If the path to a camera is selected, we follow that camera.
fn tracking_camera(ctx: &ViewerContext<'_>, space_cameras: &[SpaceCamera]) -> Option<Eye> {
    if let Selection::Instance(selected) = &ctx.rec_cfg.selection {
        find_camera(space_cameras, selected)
    } else {
        None
    }
}

fn find_camera(space_cameras: &[SpaceCamera], needle: &InstanceId) -> Option<Eye> {
    let mut found_camera = None;

    for camera in space_cameras {
        if needle.obj_path == camera.camera_obj_path
            && camera.instance_index_hash == needle.instance_index_hash()
        {
            if found_camera.is_some() {
                return None; // More than one camera
            } else {
                found_camera = Some(camera);
            }
        }
    }

    found_camera.and_then(Eye::from_camera)
}

fn click_object(
    ctx: &mut ViewerContext<'_>,
    space_cameras: &[SpaceCamera],
    state: &mut View3DState,
    instance_id: &InstanceId,
) {
    ctx.rec_cfg.selection = crate::Selection::Instance(instance_id.clone());

    if let Some(camera) = find_camera(space_cameras, instance_id) {
        state.interpolate_to_eye(camera);
    } else if let Some(clicked_point) = state.hovered_point {
        // center camera on what we click on
        if let Some(mut new_orbit_eye) = state.orbit_eye {
            new_orbit_eye.orbit_radius = new_orbit_eye.position().distance(clicked_point);
            new_orbit_eye.orbit_center = clicked_point;
            state.interpolate_to_orbit_eye(new_orbit_eye);
        }
    }
}

// ----------------------------------------------------------------------------

pub(crate) const HELP_TEXT: &str = "Drag to rotate.\n\
    Drag with secondary mouse button to pan.\n\
    Drag with middle mouse button to roll the view.\n\
    Scroll to zoom.\n\
    \n\
    While hovering the 3D view, navigate with WSAD and QE.\n\
    CTRL slows down, SHIFT speeds up.\n\
    \n\
    Click on a object to focus the view on it.\n\
    \n\
    Double-click anywhere to reset the view.";

pub(crate) fn view_3d(
    ctx: &mut ViewerContext<'_>,
    ui: &mut egui::Ui,
    state: &mut View3DState,
    space: Option<&ObjPath>,
    mut scene: Scene3D,
    space_cameras: &[SpaceCamera],
) -> egui::Response {
    crate::profile_function!();

    state.scene_bbox = state.scene_bbox.union(scene.calc_bbox());
    state.space_camera = space_cameras.to_vec();

    let (rect, mut response) =
        ui.allocate_at_least(ui.available_size(), egui::Sense::click_and_drag());

    let tracking_camera = tracking_camera(ctx, space_cameras);
    let orbit_eye = state.update_eye(ctx, tracking_camera, &response);

    let did_interact_wth_eye = orbit_eye.interact(&response);
    let orbit_eye = *orbit_eye;
    let eye = orbit_eye.to_eye();

    scene.add_cameras(ctx, &state.scene_bbox, rect.size(), &eye, space_cameras);

    if did_interact_wth_eye {
        state.last_eye_interact_time = ui.input().time;
        state.eye_interpolation = None;
        if tracking_camera.is_some() {
            ctx.rec_cfg.selection = Selection::None;
        }
    }

    let mut hovered_instance = state.hovered_instance.clone();
    if ui.input().pointer.any_click() {
        if let Some(hovered_instance) = &hovered_instance {
            click_object(ctx, space_cameras, state, hovered_instance);
        }
    } else if ui.input().pointer.any_down() {
        hovered_instance = None;
    }

    if let Some(instance_id) = &hovered_instance {
        response = response.on_hover_ui_at_pointer(|ui| {
            ctx.instance_id_button(ui, instance_id);
            crate::ui::data_ui::view_instance(ctx, ui, instance_id, crate::ui::Preview::Medium);
        });
    }

    let hovered = response
        .hover_pos()
        .and_then(|pointer_pos| scene.picking(pointer_pos, &rect, &eye));

    state.hovered_instance = None;
    state.hovered_point = None;
    if let Some((instance_id, point)) = hovered {
        if let Some(instance_id) = instance_id.resolve(&ctx.log_db.obj_db.store) {
            state.hovered_instance = Some(instance_id);
            state.hovered_point = Some(point);
        }
    }

    project_onto_other_spaces(ctx, space_cameras, state, space, &response, orbit_eye);
    show_projections_from_2d_space(ctx, space_cameras, state, &mut scene);

    {
        let orbit_center_alpha = egui::remap_clamp(
            ui.input().time - state.last_eye_interact_time,
            0.0..=0.4,
            0.7..=0.0,
        ) as f32;

        if orbit_center_alpha > 0.0 {
            // Show center of orbit camera when interacting with camera (it's quite helpful).
            scene.points.push(Point3D {
                instance_id_hash: InstanceIdHash::NONE,
                pos: orbit_eye.orbit_center,
                radius: Size::new_scene(orbit_eye.orbit_radius * 0.01),
                color: [255, 0, 255, (orbit_center_alpha * 255.0) as u8],
            });
            ui.ctx().request_repaint(); // let it fade out
        }
    }

    scene.finalize_sizes_and_colors(
        rect.size(),
        &eye,
        hovered_instance.map_or(InstanceIdHash::NONE, |id| id.hash()),
    );

    paint_view(
        ui,
        eye,
        rect,
        &scene,
        state,
        ctx.render_ctx,
        &space.map_or("<unnamed>".to_owned(), |space| space.to_string()),
    );

    response
}

fn paint_view(
    ui: &mut egui::Ui,
    eye: Eye,
    rect: egui::Rect,
    scene: &Scene3D,
    state: &mut View3DState,
    render_ctx: &mut RenderContext,
    name: &str,
) {
    crate::profile_function!();

    // Draw labels:
    ui.with_layer_id(
        egui::LayerId::new(egui::Order::Foreground, egui::Id::new("LabelsLayer")),
        |ui| {
            crate::profile_function!("labels");
            let ui_from_world = eye.ui_from_world(&rect);
            for label in &scene.labels {
                let pos_in_ui = ui_from_world * label.origin.extend(1.0);
                if pos_in_ui.w <= 0.0 {
                    continue; // behind camera
                }
                let pos_in_ui = pos_in_ui / pos_in_ui.w;

                let font_id = egui::TextStyle::Monospace.resolve(ui.style());

                let galley = ui.fonts().layout(
                    (*label.text).to_owned(),
                    font_id,
                    ui.style().visuals.text_color(),
                    100.0,
                );

                let text_rect = egui::Align2::CENTER_TOP.anchor_rect(egui::Rect::from_min_size(
                    egui::pos2(pos_in_ui.x, pos_in_ui.y),
                    galley.size(),
                ));

                let bg_rect = text_rect.expand2(egui::vec2(6.0, 2.0));
                ui.painter().add(egui::Shape::rect_filled(
                    bg_rect,
                    3.0,
                    ui.style().visuals.code_bg_color,
                ));
                ui.painter().add(egui::Shape::galley(text_rect.min, galley));
            }
        },
    );

    // Determine view port resolution and position.
    let (resolution_in_pixel, origin_in_pixel) = {
        let ppp = ui.ctx().pixels_per_point();
        let min = (rect.min.to_vec2() * ppp).round();
        let max = (rect.max.to_vec2() * ppp).round();

        let resolution = max - min;
        let origin = min;

        (
            [resolution.x as u32, resolution.y as u32],
            [origin.x as u32, origin.y as u32],
        )
    };
    if resolution_in_pixel[0] == 0 || resolution_in_pixel[1] == 0 {
        return;
    }

    let (view_builder, command_buffer) = {
        crate::profile_scope!("build command buffer for view");

        let mut view_builder = ViewBuilder::default();
        view_builder
            .setup_view(
                render_ctx,
                TargetConfiguration {
                    name: name.into(),

                    resolution_in_pixel,
                    origin_in_pixel,

                    view_from_world: eye.world_from_view.inverse(),
                    fov_y: eye.fov_y,
                    near_plane_distance: eye.near(),
                },
            )
            .unwrap()
            .queue_draw(&GenericSkyboxDrawable::new(render_ctx))
            .queue_draw(&MeshDrawable::new(render_ctx, &scene.meshes()).unwrap())
            .queue_draw(
                &LineDrawable::new(render_ctx, &scene.line_strips(state.show_axes)).unwrap(),
            )
            .queue_draw(&PointCloudDrawable::new(render_ctx, &scene.point_cloud_points()).unwrap());

        let command_buffer = view_builder.draw(render_ctx).unwrap();
        (view_builder, command_buffer)
    };

    // egui paint callback are copyable / not a FnOnce (this in turn is because egui primitives can be callbacks and are copyable)
    let command_buffer = std::sync::Arc::new(egui::mutex::Mutex::new(Some(command_buffer)));
    let view_builder = std::sync::Arc::new(egui::mutex::Mutex::new(Some(view_builder)));
    ui.painter().add(egui::PaintCallback {
        rect,
        callback: std::sync::Arc::new(
            egui_wgpu::CallbackFn::new()
                .prepare(
                    move |_device, _queue, _encoder, _paint_callback_resources| {
                        let mut command_buffer = command_buffer.lock();
                        vec![std::mem::replace(&mut *command_buffer, None)
                            .expect("egui_wgpu prepare callback called more than once")]
                    },
                )
                .paint(move |_info, render_pass, paint_callback_resources| {
                    crate::profile_scope!("paint");
                    let ctx = paint_callback_resources.get().unwrap();
                    let mut view_builder = view_builder.lock();
                    std::mem::replace(&mut *view_builder, None)
                        .expect("egui_wgpu paint callback called more than once")
                        .composite(ctx, render_pass)
                        .unwrap();
                }),
        ),
    });
}

fn show_projections_from_2d_space(
    ctx: &mut ViewerContext<'_>,
    space_cameras: &[SpaceCamera],
    state: &mut View3DState,
    scene: &mut Scene3D,
) {
    if let HoveredSpace::TwoD { space_2d, pos } = &ctx.rec_cfg.hovered_space_previous_frame {
        for cam in space_cameras {
            if &cam.target_space == space_2d {
                if let Some(ray) = cam.unproject_as_ray(glam::vec2(pos.x, pos.y)) {
                    // TODO(emilk): better visualization of a ray
                    let mut hit_pos = None;
                    if pos.z.is_finite() && pos.z > 0.0 {
                        if let Some(world_from_image) = cam.world_from_image() {
                            let pos = world_from_image
                                .transform_point3(glam::vec3(pos.x, pos.y, 1.0) * pos.z);
                            hit_pos = Some(pos);
                        }
                    }
                    let length = if let Some(hit_pos) = hit_pos {
                        hit_pos.distance(cam.position())
                    } else {
                        4.0 * state.scene_bbox.half_size().length() // should be long enough
                    };
                    let origin = ray.point_along(0.0);
                    let end = ray.point_along(length);
                    let radius = Size::new_ui(1.5);
                    scene.line_strips.push(LineStrip {
                        instance_id_hash: InstanceIdHash::NONE,
                        line_strip: re_renderer::renderer::LineStrip {
                            points: smallvec![origin, end],
                            radius: radius.0,
                            color: [255; 4],
                            flags: Default::default(),
                        },
                    });

                    if let Some(pos) = hit_pos {
                        // Show where the ray hits the depth map:
                        scene.points.push(Point3D {
                            instance_id_hash: InstanceIdHash::NONE,
                            pos,
                            radius: radius * 3.0,
                            color: [255; 4],
                        });
                    }
                }
            }
        }
    }
}

fn project_onto_other_spaces(
    ctx: &mut ViewerContext<'_>,
    space_cameras: &[SpaceCamera],
    state: &mut View3DState,
    space: Option<&ObjPath>,
    response: &egui::Response,
    orbit_eye: OrbitEye,
) {
    let Some(pos_in_ui) = response.hover_pos() else { return };

    let ray_in_world = {
        let eye = orbit_eye.to_eye();
        let world_from_ui = eye.world_from_ui(&response.rect);
        let ray_origin = eye.pos_in_world();
        let ray_dir =
            world_from_ui.project_point3(glam::vec3(pos_in_ui.x, pos_in_ui.y, 1.0)) - ray_origin;
        Ray3::from_origin_dir(ray_origin, ray_dir.normalize())
    };

    let mut target_spaces = vec![];
    for cam in space_cameras {
        if let Some(target_space) = cam.target_space.clone() {
            let ray_in_2d = cam
                .image_from_world()
                .map(|image_from_world| (image_from_world * ray_in_world).normalize());

            let point_in_2d = state
                .hovered_point
                .and_then(|hovered_point| cam.project_onto_2d(hovered_point));

            target_spaces.push((target_space, ray_in_2d, point_in_2d));
        }
    }
    ctx.rec_cfg.hovered_space_this_frame = HoveredSpace::ThreeD {
        space_3d: space.cloned(),
        target_spaces,
    }
}

fn default_eye(scene_bbox: &macaw::BoundingBox, space_specs: &SpaceSpecs) -> OrbitEye {
    let mut center = scene_bbox.center();
    if !center.is_finite() {
        center = Vec3::ZERO;
    }

    let mut radius = 2.0 * scene_bbox.half_size().length();
    if !radius.is_finite() || radius == 0.0 {
        radius = 1.0;
    }

    let look_up = space_specs.up.unwrap_or(Vec3::Z);

    let look_dir = if let Some(right) = space_specs.right {
        // Make sure right is to the right, and up is up:
        let fwd = look_up.cross(right);
        0.75 * fwd + 0.25 * right - 0.25 * look_up
    } else {
        // Look along the cardinal directions:
        let look_dir = vec3(1.0, 1.0, 1.0);

        // Make sure the eye is looking down, but just slightly:
        look_dir + look_up * (-0.5 - look_dir.dot(look_up))
    };

    let look_dir = look_dir.normalize();

    let eye_pos = center - radius * look_dir;

    OrbitEye {
        orbit_center: center,
        orbit_radius: radius,
        world_from_view_rot: Quat::from_affine3(
            &Affine3A::look_at_rh(eye_pos, center, look_up).inverse(),
        ),
        fov_y: Eye::DEFAULT_FOV_Y,
        up: space_specs.up.unwrap_or(Vec3::ZERO),
        velocity: Vec3::ZERO,
    }
}