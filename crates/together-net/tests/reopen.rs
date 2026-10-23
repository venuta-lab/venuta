//! A reopened room's way in (WP15): the endpoint comes back with its key, so the same endpoint
//! id, and on the UDP port it had when that is free, so guests still reconnecting to the
//! addresses they know (Local only has no address lookup) find it.

use together_net::net::{self, Reach};

fn local() -> Reach {
    Reach {
        local_only: true,
        ..Reach::default()
    }
}

#[tokio::test]
async fn an_endpoint_comes_back_with_its_key_on_its_port() {
    let key = iroh::SecretKey::from_bytes(&[7u8; 32]);
    let first = net::endpoint(&local(), true, Some(key.clone()))
        .await
        .unwrap();
    let (id, port) = (first.id(), net::port_of(&first).expect("an IPv4 port"));
    first.close().await;
    drop(first);
    let again = net::endpoint_on(&local(), true, Some(key.clone()), Some(port))
        .await
        .unwrap();
    assert_eq!(again.id(), id, "the same endpoint id");
    assert_eq!(net::port_of(&again), Some(port), "on the port it had");
    // The port taken: on another, still the same id.
    let other = net::endpoint_on(&local(), true, Some(key), Some(port))
        .await
        .unwrap();
    assert_eq!(other.id(), id);
    assert_ne!(net::port_of(&other), Some(port));
    other.close().await;
    again.close().await;
}

#[test]
fn a_rooms_key_goes_through_its_record() {
    let (k, hex) = net::room_key(None);
    assert_eq!(hex.len(), 64);
    let (again, hex2) = net::room_key(Some(&hex));
    assert_eq!((again.public(), hex2), (k.public(), hex.clone()));
    let (other, _) = net::room_key(Some("not a key"));
    assert_ne!(
        other.public(),
        k.public(),
        "a new one when the kept one doesn't read"
    );
}
