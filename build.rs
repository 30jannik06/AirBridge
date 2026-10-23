// Cargo führt build.rs VOR dem eigentlichen Kompilieren aus.
// Hier kompilieren wir den C-Code von playfair (aus UxPlay) mit und linken ihn statisch ein.
fn main() {
    cc::Build::new()
        .files([
            "vendor/playfair/playfair.c",
            "vendor/playfair/hand_garble.c",
            "vendor/playfair/modified_md5.c",
            "vendor/playfair/omg_hax.c",
            "vendor/playfair/sap_hash.c",
            "vendor/fp_tables.c",
        ])
        .warnings(false) // fremder Code, wir warten ihn nicht
        .compile("playfair");

    println!("cargo:rerun-if-changed=vendor");
}
