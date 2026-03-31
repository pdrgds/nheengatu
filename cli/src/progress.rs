use indicatif::{ProgressBar, ProgressStyle};

pub fn create_progress_bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:30}] {pos}/{len} chapters")
            .expect("invalid progress bar template")
            .progress_chars("=>-"),
    );
    pb
}
