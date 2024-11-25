use std::fs::{self};
use std::path::Path;

fn main() {
    let src = "../solana-programs/target/idls";
    let dest = "idl";

    // Copy the contents of the directory
    if let Ok(entries) = fs::read_dir(src) {
        for entry in entries.filter_map(Result::ok) {
            let src_path = entry.path();
            let dest_path = Path::new(dest).join(src_path.file_name().unwrap());
            fs::copy(&src_path, &dest_path).expect("Failed to copy file");
        }
    }
}
