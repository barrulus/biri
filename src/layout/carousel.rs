use smithay::utils::{Logical, Point, Rectangle, Size};

/// A single sibling card's destination in the host overview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardPlacement {
    /// Destination box in HOST logical coords (also the crop box).
    pub card_rect: Rectangle<f64, Logical>,
    /// Shrink factor applied to the sibling before scale-normalization.
    pub card_scale: f64,
}

/// Tuck up to `n_cards` sibling previews around the host overview: the first
/// tucks against the left edge, the second against the right, further cards
/// step inward, all vertically centered. Deterministic in card index.
pub fn carousel_card_layout(view_size: Size<f64, Logical>, n_cards: usize) -> Vec<CardPlacement> {
    if n_cards == 0 {
        return Vec::new();
    }
    // A card occupies ~28% of the host width, vertically centered.
    let card_scale = 0.28;
    let card_w = view_size.w * card_scale;
    let card_h = view_size.h * card_scale;
    let y = (view_size.h - card_h) / 2.;
    let margin = view_size.w * 0.02;

    (0..n_cards)
        .map(|i| {
            // Alternate sides; each further pair steps inward by one card width.
            let pair = (i / 2) as f64;
            let step = (card_w + margin) * pair;
            let x = if i % 2 == 0 {
                margin + step
            } else {
                view_size.w - margin - card_w - step
            };
            // Guarantee containment for arbitrary n_cards: clamp so the whole
            // card box stays within the host view (cards may pile up at the
            // edges for very large counts, which is acceptable).
            let x = x.clamp(0.0, (view_size.w - card_w).max(0.0));
            CardPlacement {
                card_rect: Rectangle::new(Point::from((x, y)), Size::from((card_w, card_h))),
                card_scale,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::utils::Size;

    #[test]
    fn zero_cards_is_empty() {
        assert!(carousel_card_layout(Size::from((1920., 1080.)), 0).is_empty());
    }

    #[test]
    fn cards_are_tucked_left_and_right_and_stay_on_screen() {
        let view = Size::from((1920., 1080.));
        let cards = carousel_card_layout(view, 2);
        assert_eq!(cards.len(), 2);
        // First card tucks left, second tucks right.
        assert!(cards[0].card_rect.loc.x < view.w / 2.);
        assert!(cards[1].card_rect.loc.x > view.w / 2.);
        // Every card box stays fully within the host view (no off-screen).
        for c in &cards {
            assert!(c.card_rect.loc.x >= 0.);
            assert!(c.card_rect.loc.y >= 0.);
            assert!(c.card_rect.loc.x + c.card_rect.size.w <= view.w);
            assert!(c.card_rect.loc.y + c.card_rect.size.h <= view.h);
            assert!(c.card_scale > 0. && c.card_scale < 1.);
        }
    }

    #[test]
    fn many_cards_stay_on_screen() {
        let view = Size::from((1920., 1080.));
        for c in carousel_card_layout(view, 8) {
            assert!(c.card_rect.loc.x >= 0.);
            assert!(c.card_rect.loc.x + c.card_rect.size.w <= view.w);
            assert!(c.card_rect.loc.y >= 0.);
            assert!(c.card_rect.loc.y + c.card_rect.size.h <= view.h);
        }
    }
}
