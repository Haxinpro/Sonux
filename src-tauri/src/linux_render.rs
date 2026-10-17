//! WebKitGTK rendering environment, set before the web view starts.

use std::path::Path;

/// The parts of the host that decide the rendering workaround.
pub struct Host {
    pub nvidia: bool,
    pub wayland: bool,
}

/// Variables to set (name, value). Variables the user already set are kept.
pub fn plan(host: &Host) -> Vec<(&'static str, &'static str)> {
    if host.nvidia && host.wayland {
        // WebKitGTK 2.54 glitched the (then transparent) window here when
        // DMABUF is off, but with it on the NVIDIA driver raises Gdk Error 71
        // unless explicit sync is off. The window is opaque now; this setup is
        // the one measured. Checked on one NVIDIA + KWin setup only.
        vec![("__NV_DISABLE_EXPLICIT_SYNC", "1")]
    } else {
        // DMABUF can raise Gdk Error 71 on other GPU/driver combinations.
        vec![("WEBKIT_DISABLE_DMABUF_RENDERER", "1")]
    }
}

/// True when GTK will use Wayland: a Wayland display exists and
/// `GDK_BACKEND`, if set, lists `wayland` first.
fn is_wayland(display: bool, gdk_backend: Option<&str>) -> bool {
    display && gdk_backend.is_none_or(|b| b.split(',').next() == Some("wayland"))
}

/// True when the NVIDIA module is loaded and an NVIDIA card is the boot GPU.
/// A loaded module alone is not enough: on hybrid laptops the web view may
/// render on Intel or AMD.
fn nvidia_boot_gpu() -> bool {
    let boot_is_nvidia = |card: std::fs::DirEntry| {
        let dev = card.path().join("device");
        let read = |name| std::fs::read_to_string(dev.join(name)).unwrap_or_default();
        read("boot_vga").trim() == "1" && read("vendor").trim() == "0x10de"
    };
    Path::new("/proc/driver/nvidia").exists()
        && std::fs::read_dir("/sys/class/drm")
            .is_ok_and(|mut cards| cards.any(|card| card.is_ok_and(boot_is_nvidia)))
}

/// Apply `plan` for this machine. Call first thing in `main`.
pub fn configure() {
    let gdk = std::env::var("GDK_BACKEND").ok();
    let host = Host {
        nvidia: nvidia_boot_gpu(),
        wayland: is_wayland(
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
            gdk.as_deref(),
        ),
    };
    for (name, value) in plan(&host) {
        if std::env::var_os(name).is_none() {
            std::env::set_var(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DMABUF_OFF: (&str, &str) = ("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    const SYNC_OFF: (&str, &str) = ("__NV_DISABLE_EXPLICIT_SYNC", "1");

    #[test]
    fn nvidia_wayland_keeps_dmabuf() {
        let p = plan(&Host {
            nvidia: true,
            wayland: true,
        });
        assert_eq!(p, vec![SYNC_OFF]);
    }

    #[test]
    fn nvidia_x11_disables_dmabuf() {
        let p = plan(&Host {
            nvidia: true,
            wayland: false,
        });
        assert_eq!(p, vec![DMABUF_OFF]);
    }

    #[test]
    fn other_gpus_disable_dmabuf() {
        for wayland in [true, false] {
            let p = plan(&Host {
                nvidia: false,
                wayland,
            });
            assert_eq!(p, vec![DMABUF_OFF]);
        }
    }

    #[test]
    fn gtk_backend_decides_wayland() {
        assert!(is_wayland(true, None));
        assert!(is_wayland(true, Some("wayland")));
        assert!(is_wayland(true, Some("wayland,x11")));
        assert!(!is_wayland(true, Some("x11")));
        assert!(!is_wayland(true, Some("x11,wayland")));
        assert!(!is_wayland(false, None));
    }
}
