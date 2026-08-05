use crate::ui::conversation::diagram::cache::MermaidCache;
use crate::ui::conversation::diagram::halfblock::png_to_halfblock_lines;
use crate::ui::conversation::diagram::layout::compute_layout_from_png_bytes;
use crate::ui::conversation::diagram::mmdc::{MermaidRenderConfig, render_with_mmdc};
use crate::ui::conversation::diagram::slot::DiagramPhase;
use crate::ui::conversation::document::{ConversationDocument, DocBlock};
use image::GenericImageView;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

pub struct RenderFinished {
    pub hash: String,
    pub result: DiagramPhase,
}

pub struct DiagramScheduler {
    cfg: MermaidRenderConfig,
    in_flight: Arc<Mutex<HashSet<String>>>,
    tx: mpsc::Sender<RenderFinished>,
    rx: mpsc::Receiver<RenderFinished>,
}

impl DiagramScheduler {
    pub fn new(cfg: MermaidRenderConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            cfg,
            in_flight: Arc::new(Mutex::new(HashSet::new())),
            tx,
            rx,
        }
    }

    pub fn tick(&mut self, doc: &ConversationDocument, scroll: usize, vp: usize, width: u16) {
        let prefetch = self.cfg.prefetch_viewports;
        let window_start = scroll;
        let window_end = scroll + vp * (1 + prefetch);

        let mut current_row = 0;
        for block in &doc.blocks {
            let height = match block {
                DocBlock::Text(lines) => lines.len(),
                DocBlock::Diagram(slot) => slot.row_height,
            };

            if let DocBlock::Diagram(slot) = block {
                let intersects = current_row < window_end && current_row + height > window_start;

                if intersects {
                    let needs_render = match &slot.phase {
                        DiagramPhase::Pending => true,
                        DiagramPhase::Ready { width: w, .. } => *w != width,
                        _ => false,
                    };

                    if needs_render {
                        self.start_render(slot.hash.clone(), slot.source.clone(), width);
                    }
                }
            }

            current_row += height;
        }
    }

    fn start_render(&self, hash: String, source: String, width: u16) {
        let mut in_flight = self.in_flight.lock().unwrap();
        if in_flight.contains(&hash) {
            return;
        }
        in_flight.insert(hash.clone());

        let tx = self.tx.clone();
        let cfg = self.cfg.clone();
        let hash_cloned = hash.clone();
        let in_flight_shared = self.in_flight.clone();
        let cache = MermaidCache::new(cfg.cache_dir.clone());

        thread::spawn(move || {
            let result: anyhow::Result<DiagramPhase> = (|| {
                // Check cache first
                let png_bytes = if let Some(png) = cache.get(&hash_cloned) {
                    png
                } else {
                    let png = render_with_mmdc(&source, &cfg)?;
                    let _ = cache.put(&hash_cloned, &png);
                    png
                };

                let layout = compute_layout_from_png_bytes(&png_bytes, width, cfg.max_rows)?;
                let halfblocks = png_to_halfblock_lines(&png_bytes, &layout)?;
                let (png_w, png_h) = image::load_from_memory(&png_bytes)?.dimensions();

                Ok(DiagramPhase::Ready {
                    png: Arc::new(png_bytes),
                    png_w,
                    png_h,
                    width,
                    halfblocks,
                    row_height: layout.row_height,
                    col_width: layout.col_width,
                })
            })();

            let phase = match result {
                Ok(p) => p,
                Err(e) => DiagramPhase::Failed {
                    message: e.to_string(),
                    fallback: vec![], // TODO: populate fallback
                },
            };

            let _ = tx.send(RenderFinished {
                hash: hash_cloned.clone(),
                result: phase,
            });

            in_flight_shared.lock().unwrap().remove(&hash_cloned);
        });
    }

    pub fn poll_completions(&mut self) -> Vec<RenderFinished> {
        let mut results = Vec::new();
        while let Ok(finished) = self.rx.try_recv() {
            results.push(finished);
        }
        results
    }
}
