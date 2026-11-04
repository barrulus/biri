use niri_config::Config;
use smithay::utils::Point;

use super::*;

fn setup(warp: bool) -> (Fixture, (f64, f64)) {
    let input = if warp {
        "input { warp-mouse-to-focus; }"
    } else {
        ""
    };
    let config = Config::parse_mem(&format!(
        "{input}\noverview {{ zoom 0.15; consolidated-carousel {{ reveal-zoom 0.4; assembled-zoom 0.15; }}; }}"
    )).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));
    let id = f.add_client();
    for output in [1, 2] {
        f.niri_focus_output(output);
        let window = f.client(id).create_window();
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(id);
        let window = f.client(id).window(&surface);
        window.attach_new_buffer();
        window.set_size(100, 100);
        window.ack_last_and_commit();
        f.double_roundtrip(id);
        f.niri_complete_animations();
    }

    let target = f.niri_output(2);
    let rect = f
        .niri()
        .layout
        .monitor_for_output(&target)
        .unwrap()
        .active_window_visual_rectangle()
        .unwrap();
    f.niri_focus_output(1);
    f.niri_state().move_cursor(Point::from((100., 100.)));
    f.niri().layout.toggle_overview();
    f.niri_complete_animations();
    f.niri().layout.carousel_request_rotate(1);
    f.niri_complete_animations();
    assert!(f.niri().layout.in_carousel_lens());
    f.niri_state().update_keyboard_focus();
    assert!(f.niri().keyboard_focus.is_overview());

    let layout = &f.niri().layout;
    let zoom = layout.overview_zoom();
    let monitor = layout.monitor_for_output(&target).unwrap();
    let (_, geo) = monitor
        .workspaces_with_render_geo_at_zoom(zoom)
        .find(|(ws, _)| ws.id() == monitor.active_workspace_ref().id())
        .unwrap();
    let center =
        geo.loc + (rect.loc + Point::from((rect.size.w / 2., rect.size.h / 2.))).upscale(zoom);
    let view = monitor.view_size();
    (f, (center.x / view.w, center.y / view.h))
}

#[test]
fn carousel_confirmation_moves_pointer_to_selected_output() {
    for warp in [false, true] {
        for click in [false, true] {
            let (mut f, uv) = setup(warp);
            let target = f.niri_output(2);
            assert!(f
                .niri_state()
                .confirm_carousel_focus_jump(click.then_some(uv)));
            let niri = f.niri();
            assert_eq!(niri.layout.active_output(), Some(&target));
            assert!(!niri.layout.is_overview_open());
            assert!(niri.keyboard_focus.is_layout());
            let location = niri.seat.get_pointer().unwrap().current_location();
            let geo = niri.global_space.output_geometry(&target).unwrap();
            assert!(geo.to_f64().contains(location));
            if warp {
                let rect = niri
                    .layout
                    .monitor_for_output(&target)
                    .unwrap()
                    .active_window_visual_rectangle()
                    .unwrap();
                assert!(
                    smithay::utils::Rectangle::new(rect.loc + geo.loc.to_f64(), rect.size)
                        .contains(location)
                );
            }
        }
    }
}

#[test]
fn carousel_miss_does_not_move_pointer_or_close_overview() {
    let (mut f, _) = setup(true);
    let output = f.niri().layout.active_output().cloned();
    let location = f.niri().seat.get_pointer().unwrap().current_location();
    assert!(!f.niri_state().confirm_carousel_focus_jump(Some((-1., -1.))));
    assert_eq!(f.niri().layout.active_output(), output.as_ref());
    assert!(f.niri().layout.is_overview_open());
    assert_eq!(
        f.niri().seat.get_pointer().unwrap().current_location(),
        location
    );
}

#[test]
fn carousel_enter_moves_pointer_to_selected_output() {
    use smithay::backend::input::{InputEvent, InputTime, KeyState, Keycode};

    use super::test_input_backend::{TestInputBackend, TestKeyboardKeyEvent};

    let (mut f, _) = setup(false);
    let target = f.niri_output(2);
    f.niri_state()
        .process_input_event(InputEvent::<TestInputBackend>::Keyboard {
            event: TestKeyboardKeyEvent {
                time: InputTime::from_micros(0),
                code: Keycode::new(36), // XKB Return.
                state: KeyState::Pressed,
                count: 1,
            },
        });
    assert_eq!(f.niri().layout.active_output(), Some(&target));
    let location = f.niri().seat.get_pointer().unwrap().current_location();
    assert!(f
        .niri()
        .global_space
        .output_geometry(&target)
        .unwrap()
        .to_f64()
        .contains(location));
}

#[test]
fn carousel_click_moves_pointer_to_selected_output() {
    use wayland_client::protocol::wl_pointer;

    let (mut f, uv) = setup(false);
    let host = f.niri_output(1);
    let target = f.niri_output(2);
    let corners = [[50., 50.], [150., 50.], [150., 150.], [50., 150.]];
    f.niri()
        .output_state
        .get(&host)
        .unwrap()
        .panel_hits
        .replace(vec![(1., corners)]);
    f.niri_state()
        .move_cursor(Point::from((50. + 100. * uv.0, 50. + 100. * uv.1)));
    let id = f.state.clients[0].id;
    f.client(id)
        .state
        .virtual_pointer()
        .button(0, 0x110, wl_pointer::ButtonState::Pressed);
    f.double_roundtrip(id);
    assert_eq!(f.niri().layout.active_output(), Some(&target));
    let location = f.niri().seat.get_pointer().unwrap().current_location();
    assert!(f
        .niri()
        .global_space
        .output_geometry(&target)
        .unwrap()
        .to_f64()
        .contains(location));
}
