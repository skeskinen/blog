use std::fs::File;
use std::io::Write;
use std::path::Path;

use sass_rs;

const SCSS_FILE: &str = "src/style.scss";
const CSS_FILE: &str = "static/style.css";

fn main() {
    let scss_file_path = Path::new(SCSS_FILE);
    let css_file_path = Path::new(CSS_FILE);
    let scss_modified = scss_file_path.metadata().unwrap().modified().unwrap();
    let css_modified = css_file_path.metadata().unwrap().modified().unwrap();
    if scss_modified > css_modified {
        let mut css_file = File::create(css_file_path).unwrap();
        let content = sass_rs::compile_file(scss_file_path, Default::default()).unwrap();
        css_file.write_all(content.as_bytes()).unwrap();
    }
}