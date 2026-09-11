fn main() {
    let protocols = [
        "protocols/river-window-management-v1.xml",
        "protocols/river-layer-shell-v1.xml",
        "protocols/river-xkb-bindings-v1.xml",
        "protocols/river-input-management-v1.xml",
        "protocols/river-libinput-config-v1.xml",
        "protocols/river-xkb-config-v1.xml",
    ];

    for protocol in protocols {
        println!("cargo:rerun-if-changed={protocol}");
    }
}
