//! `CosmicEiSockets` against a stand-in for stock cosmic-comp's `GetSenderSocket` on a private
//! bus: it enforces what stock enforces (the caller owns `com.system76.CosmicOSK`), so the test
//! shows the name workaround is what lets the call through.

use crate::support::bus::PrivateBus;
use cua_desktop::{DesktopError, DeviceTypes, SenderSocket};
use cuad::{CosmicEiSockets, EI_BUS_NAME, EI_PATH, OSK_NAME};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use zbus::fdo;
use zbus::message::Header;
use zbus::zvariant::OwnedFd;

#[derive(Default)]
struct Seen {
    devices: Vec<u32>,
    ends: Vec<UnixStream>,
}

struct FakeEi(Arc<Mutex<Seen>>);

#[zbus::interface(name = "com.system76.CosmicComp.Ei")]
impl FakeEi {
    async fn get_sender_socket(
        &self,
        devices: u32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<OwnedFd> {
        let dbus = fdo::DBusProxy::new(connection).await?;
        let owner = dbus
            .get_name_owner(OSK_NAME.try_into().expect("a name"))
            .await;
        let caller = header.sender().map(ToString::to_string);
        if owner.ok().map(|o| o.to_string()) != caller {
            return Err(fdo::Error::AccessDenied("Access denied".into()));
        }
        let (ours, theirs) = UnixStream::pair().expect("pair");
        let mut seen = self.0.lock().expect("seen");
        seen.devices.push(devices);
        seen.ends.push(theirs);
        Ok(std::os::fd::OwnedFd::from(ours).into())
    }
}

async fn compositor(bus: &PrivateBus) -> (zbus::Connection, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let connection = bus.connect().await;
    connection
        .object_server()
        .at(EI_PATH, FakeEi(seen.clone()))
        .await
        .expect("serve");
    connection.request_name(EI_BUS_NAME).await.expect("name");
    (connection, seen)
}

#[tokio::test]
async fn the_osk_name_lets_the_call_through_and_the_socket_is_a_live_descriptor() {
    let dir = tempfile::tempdir().expect("dir");
    let bus = PrivateBus::start(dir.path());
    let (_compositor, seen) = compositor(&bus).await;
    let cuad = bus.connect().await;
    let sockets = CosmicEiSockets::over(cuad.clone());

    let fd = sockets
        .sender_socket(DeviceTypes::ALL)
        .await
        .expect("a socket");
    assert_eq!(seen.lock().unwrap().devices, vec![7]);
    let mut ours = UnixStream::from(fd);
    ours.write_all(b"ei").expect("write");
    let mut theirs = seen.lock().unwrap().ends.remove(0);
    let mut got = [0u8; 2];
    theirs.read_exact(&mut got).expect("read");
    assert_eq!(&got, b"ei");

    // The name is taken once and kept: a second call needs no second request.
    sockets
        .sender_socket(DeviceTypes::KEYBOARD)
        .await
        .expect("again");
    assert_eq!(seen.lock().unwrap().devices, vec![7, 1]);
}

#[tokio::test]
async fn nothing_is_taken_until_the_first_call() {
    let dir = tempfile::tempdir().expect("dir");
    let bus = PrivateBus::start(dir.path());
    let (_compositor, _seen) = compositor(&bus).await;
    let cuad = bus.connect().await;
    let probe = bus.connect().await;
    let _sockets = CosmicEiSockets::over(cuad);
    let dbus = fdo::DBusProxy::new(&probe).await.expect("dbus");
    assert!(
        !dbus
            .name_has_owner(OSK_NAME.try_into().expect("a name"))
            .await
            .expect("ask")
    );
}

#[tokio::test]
async fn a_name_someone_else_holds_is_a_refusal_the_caller_can_read() {
    let dir = tempfile::tempdir().expect("dir");
    let bus = PrivateBus::start(dir.path());
    let (_compositor, seen) = compositor(&bus).await;
    let osk = bus.connect().await;
    osk.request_name(OSK_NAME).await.expect("the real OSK");
    let cuad = bus.connect().await;
    let sockets = CosmicEiSockets::over(cuad);

    let error = sockets.sender_socket(DeviceTypes::ALL).await.unwrap_err();
    let DesktopError::Protocol(text) = error else {
        panic!("{error:?}");
    };
    assert!(text.contains("GetSenderSocket"), "{text}");
    assert!(seen.lock().unwrap().devices.is_empty());
}

#[tokio::test]
async fn no_compositor_on_the_bus_is_a_refusal_too() {
    let dir = tempfile::tempdir().expect("dir");
    let bus = PrivateBus::start(dir.path());
    let cuad = bus.connect().await;
    let sockets = CosmicEiSockets::over(cuad);
    assert!(matches!(
        sockets.sender_socket(DeviceTypes::ALL).await,
        Err(DesktopError::Protocol(_))
    ));
}
