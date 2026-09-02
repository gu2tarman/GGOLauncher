use serde::Serialize;
use std::fmt;

/// Signed physical-pixel rectangle.
/// `left`/`top` are inclusive and `right`/`bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SignedRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl SignedRect {
    pub fn width(self) -> i64 {
        i64::from(self.right) - i64::from(self.left)
    }

    pub fn height(self) -> i64 {
        i64::from(self.bottom) - i64::from(self.top)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GridCell {
    pub slot: &'static str,
    pub rect: SignedRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutError {
    NonPositiveWidth { left: i32, right: i32 },
    NonPositiveHeight { top: i32, bottom: i32 },
    CoordinateOutOfRange,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonPositiveWidth { left, right } => {
                write!(f, "RECT width must be positive: left={left}, right={right}")
            }
            Self::NonPositiveHeight { top, bottom } => {
                write!(
                    f,
                    "RECT height must be positive: top={top}, bottom={bottom}"
                )
            }
            Self::CoordinateOutOfRange => write!(f, "calculated coordinate is outside i32"),
        }
    }
}

impl std::error::Error for LayoutError {}

/// Split a signed work-area RECT into a gapless 2x2 grid.
///
/// Intermediate arithmetic uses i64. For odd dimensions, the remainder is
/// assigned to the right column and bottom row.
pub fn split_2x2(rect: SignedRect) -> Result<[GridCell; 4], LayoutError> {
    let width = rect.width();
    if width <= 0 {
        return Err(LayoutError::NonPositiveWidth {
            left: rect.left,
            right: rect.right,
        });
    }

    let height = rect.height();
    if height <= 0 {
        return Err(LayoutError::NonPositiveHeight {
            top: rect.top,
            bottom: rect.bottom,
        });
    }

    let middle_x = i64::from(rect.left) + width / 2;
    let middle_y = i64::from(rect.top) + height / 2;
    let middle_x = i32::try_from(middle_x).map_err(|_| LayoutError::CoordinateOutOfRange)?;
    let middle_y = i32::try_from(middle_y).map_err(|_| LayoutError::CoordinateOutOfRange)?;

    Ok([
        GridCell {
            slot: "r0c0",
            rect: SignedRect {
                left: rect.left,
                top: rect.top,
                right: middle_x,
                bottom: middle_y,
            },
        },
        GridCell {
            slot: "r0c1",
            rect: SignedRect {
                left: middle_x,
                top: rect.top,
                right: rect.right,
                bottom: middle_y,
            },
        },
        GridCell {
            slot: "r1c0",
            rect: SignedRect {
                left: rect.left,
                top: middle_y,
                right: middle_x,
                bottom: rect.bottom,
            },
        },
        GridCell {
            slot: "r1c1",
            rect: SignedRect {
                left: middle_x,
                top: middle_y,
                right: rect.right,
                bottom: rect.bottom,
            },
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_exact_partition(source: SignedRect, cells: &[GridCell; 4]) {
        assert_eq!(cells[0].rect.left, source.left);
        assert_eq!(cells[0].rect.top, source.top);
        assert_eq!(cells[1].rect.right, source.right);
        assert_eq!(cells[2].rect.left, source.left);
        assert_eq!(cells[2].rect.bottom, source.bottom);
        assert_eq!(cells[3].rect.right, source.right);
        assert_eq!(cells[3].rect.bottom, source.bottom);

        assert_eq!(cells[0].rect.right, cells[1].rect.left);
        assert_eq!(cells[2].rect.right, cells[3].rect.left);
        assert_eq!(cells[0].rect.bottom, cells[2].rect.top);
        assert_eq!(cells[1].rect.bottom, cells[3].rect.top);

        let source_area = i128::from(source.width()) * i128::from(source.height());
        let cells_area: i128 = cells
            .iter()
            .map(|cell| i128::from(cell.rect.width()) * i128::from(cell.rect.height()))
            .sum();
        assert_eq!(cells_area, source_area);
        assert!(cells
            .iter()
            .all(|cell| cell.rect.width() > 0 && cell.rect.height() > 0));
    }

    #[test]
    fn preserves_negative_monitor_coordinates() {
        let source = SignedRect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1040,
        };
        let cells = split_2x2(source).unwrap();

        assert_eq!(cells[0].rect.left, -1920);
        assert_eq!(cells[1].rect.right, 0);
        assert_exact_partition(source, &cells);
    }

    #[test]
    fn assigns_odd_remainders_to_right_and_bottom() {
        let source = SignedRect {
            left: 10,
            top: 20,
            right: 15,
            bottom: 23,
        };
        let cells = split_2x2(source).unwrap();

        assert_eq!(cells[0].rect.width(), 2);
        assert_eq!(cells[1].rect.width(), 3);
        assert_eq!(cells[0].rect.height(), 1);
        assert_eq!(cells[2].rect.height(), 2);
        assert_exact_partition(source, &cells);
    }

    #[test]
    fn handles_negative_origin_and_odd_size_together() {
        let source = SignedRect {
            left: -101,
            top: -51,
            right: -90,
            bottom: -40,
        };
        let cells = split_2x2(source).unwrap();

        assert_eq!(cells[0].rect.width(), 5);
        assert_eq!(cells[1].rect.width(), 6);
        assert_eq!(cells[0].rect.height(), 5);
        assert_eq!(cells[2].rect.height(), 6);
        assert_exact_partition(source, &cells);
    }

    #[test]
    fn uses_i64_for_extreme_signed_coordinates() {
        let source = SignedRect {
            left: i32::MIN,
            top: i32::MIN,
            right: i32::MAX,
            bottom: i32::MAX,
        };
        let cells = split_2x2(source).unwrap();

        assert_eq!(source.width(), 4_294_967_295);
        assert_eq!(source.height(), 4_294_967_295);
        assert_exact_partition(source, &cells);
    }

    #[test]
    fn rejects_zero_or_negative_width_without_panicking() {
        for source in [
            SignedRect {
                left: 10,
                top: 0,
                right: 10,
                bottom: 10,
            },
            SignedRect {
                left: 11,
                top: 0,
                right: 10,
                bottom: 10,
            },
        ] {
            assert!(matches!(
                split_2x2(source),
                Err(LayoutError::NonPositiveWidth { .. })
            ));
        }
    }

    #[test]
    fn rejects_zero_or_negative_height_without_panicking() {
        for source in [
            SignedRect {
                left: 0,
                top: 10,
                right: 10,
                bottom: 10,
            },
            SignedRect {
                left: 0,
                top: 11,
                right: 10,
                bottom: 10,
            },
        ] {
            assert!(matches!(
                split_2x2(source),
                Err(LayoutError::NonPositiveHeight { .. })
            ));
        }
    }
}
