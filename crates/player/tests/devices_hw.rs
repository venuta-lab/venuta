//! Windows' own endpoints, read and classified (`docs/design/devices.md`): nothing is opened and
//! nothing plays, the endpoints are only listed. Ignored in CI (it needs a Windows computer with
//! sound devices); run it with
//! `cargo test -p player --test devices_hw -- --ignored --nocapture`.

#![cfg(windows)]

use player::devices::Devices;
use player::devices::{EndpointState, Kind, classify, entries, resolve};
use player::wasapi_devices::{WasapiDevices, list_endpoints};

#[test]
#[ignore = "reads this computer's sound devices"]
fn this_computers_endpoints_are_read_and_classified() {
    let (eps, default) = list_endpoints().expect("the endpoints");
    assert!(!eps.is_empty(), "some endpoint is known");
    for e in &eps {
        let c = classify(e);
        println!(
            "{:<10} {:?} {:?} hands-free {} | {} | {} | {} | form {} | {}",
            format!("{:?}", e.state),
            c.kind,
            c.form,
            c.hands_free,
            e.description,
            e.adapter,
            e.enumerator,
            e.form_factor,
            e.container.as_deref().unwrap_or("-"),
        );
        // Every endpoint says what bus found it, so none is left unknown.
        assert!(!e.enumerator.is_empty(), "{e:?}");
        assert_ne!(c.kind, Kind::Other, "{e:?}");
    }
    println!("default: {default:?}");
    let list = entries(&eps, default.as_deref(), None);
    for d in &list {
        println!(
            "{:?}: {} ({}) default {}",
            d.kind, d.name, d.detail, d.default
        );
    }
    let active = eps
        .iter()
        .filter(|e| e.state == EndpointState::Active)
        .count();
    assert!(list.len() <= active);
    if let Some(r) = resolve(None, &eps, default.as_deref(), &[]) {
        println!(
            "following Windows plays on: {} ({:?})",
            r.entry.name, r.note
        );
    }
    // The watcher starts, keeps the same listing, and stops.
    let w = WasapiDevices::new().expect("the watcher");
    assert_eq!(w.endpoints().len(), eps.len());
    assert_eq!(w.default_id(), default);
}
