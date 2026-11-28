use std::time::{Duration, Instant};

use super::app::{Session, gpu_view::ImageSink};
use model_reviewer_core::texture::{ImageId, MipChain, chain_bytes};
use model_reviewer_render::ImageProgress;

#[derive(Default)]
pub struct CountingImages {
    pub started: Vec<(ImageId, [u32; 2])>,
    pending: Option<(ImageId, u64)>,
}

impl ImageSink for CountingImages {
    fn begin_image(&mut self, id: ImageId, chain: MipChain) -> Result<(), String> {
        assert!(self.pending.is_none(), "前の画像の転送が残っています");
        let bytes = chain.levels.iter().map(|p| p.len() as u64).sum::<u64>();
        assert_eq!(bytes, chain_bytes(chain.size));
        self.started.push((id, chain.size));
        self.pending = Some((id, bytes));
        Ok(())
    }

    fn continue_images(&mut self, budget: u64) -> ImageProgress {
        let (id, left) = self.pending.as_mut().unwrap();
        let uploaded_bytes = (*left).min(budget & !3);
        *left -= uploaded_bytes;
        let completed = (*left == 0).then_some(*id);
        if completed.is_some() {
            self.pending = None;
        }
        ImageProgress {
            uploaded_bytes,
            completed,
        }
    }
}

impl CountingImages {
    pub fn finish(&mut self, session: &mut Session) {
        until(|| {
            session.poll_loader();
            session.poll_images(self, 65536);
            !session.textures_busy() && !session.is_loading()
        });
    }
}

pub fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "画像の読み込みが終わりません");
        std::thread::yield_now();
    }
}
