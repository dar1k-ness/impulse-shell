//! How a sidebar container lays out its widgets: widgets narrower than the grid fill rows of
//! `columns` cells (a new row starts when the next one does not fit, as the original's quick toggles do),
//! and each run of rows forms one grid section; a full-width widget is a block section of its own.

use super::{Container, ContainerKind, Place};

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub id: String,
    pub r#type: String,
    /// Cells taken, clamped to `1..=columns`.
    pub size: u32,
    pub settings: toml::Table,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Section {
    Grid(Vec<Vec<Cell>>),
    Block(Cell),
}

/// The grid width and sections of a sidebar container; `None` for other container kinds.
pub fn sections(c: &Container) -> Option<(u32, Vec<Section>)> {
    let ContainerKind::Sidebar(props) = &c.kind else {
        return None;
    };
    let columns = props.columns.max(1);
    let mut out = Vec::new();
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut used = 0;
    for w in &c.widgets {
        let Place::Sidebar { size } = w.place else {
            continue;
        };
        let cell = Cell {
            id: w.id.clone(),
            r#type: w.r#type.clone(),
            size: size.clamp(1, columns),
            settings: w.settings.clone(),
        };
        if cell.size == columns {
            if !rows.is_empty() {
                out.push(Section::Grid(std::mem::take(&mut rows)));
            }
            used = 0;
            out.push(Section::Block(cell));
            continue;
        }
        if rows.is_empty() || used + cell.size > columns {
            rows.push(Vec::new());
            used = 0;
        }
        used += cell.size;
        rows.last_mut().unwrap().push(cell);
    }
    if !rows.is_empty() {
        out.push(Section::Grid(rows));
    }
    Some((columns, out))
}

/// The spacing a sidebar lays its sections out with, as SidebarLayoutContent reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    /// Around a grid section's rows.
    pub cell_padding: f64,
    /// Between a grid section's rows.
    pub cell_spacing: f64,
    /// Between sections.
    pub section_spacing: f64,
}

/// The height `sections` take laid out top to bottom with `m`: a grid section its padding and
/// its rows, each as tall as its tallest cell; a block what `height` gives for its cell (for a
/// block that fills, the least it keeps). `None` while any cell has no height measured.
pub fn height(
    sections: &[Section],
    height: impl Fn(&Cell) -> Option<f64>,
    m: &Metrics,
) -> Option<f64> {
    let mut total = 0.0;
    for (i, s) in sections.iter().enumerate() {
        if i > 0 {
            total += m.section_spacing;
        }
        total += match s {
            Section::Block(c) => height(c)?,
            Section::Grid(rows) => {
                let mut h = m.cell_padding * 2.0;
                for (r, row) in rows.iter().enumerate() {
                    if r > 0 {
                        h += m.cell_spacing;
                    }
                    let mut tallest: f64 = 0.0;
                    for c in row {
                        tallest = tallest.max(height(c)?);
                    }
                    h += tallest;
                }
                h
            }
        };
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout_model::{Side, SidebarProps, Widget};

    fn sidebar(columns: u32, widgets: &[(&str, u32)]) -> Container {
        Container {
            id: "sidebar".into(),
            kind: ContainerKind::Sidebar(SidebarProps {
                side: Side::Right,
                columns,
            }),
            widgets: widgets
                .iter()
                .enumerate()
                .map(|(i, &(t, size))| Widget {
                    id: format!("w{i}"),
                    r#type: t.into(),
                    place: Place::Sidebar { size },
                    settings: toml::Table::new(),
                })
                .collect(),
        }
    }

    fn shape(s: &[Section]) -> Vec<Vec<Vec<(String, u32)>>> {
        s.iter()
            .map(|s| match s {
                Section::Grid(rows) => rows
                    .iter()
                    .map(|r| r.iter().map(|c| (c.r#type.clone(), c.size)).collect())
                    .collect(),
                Section::Block(c) => vec![vec![(format!("[{}]", c.r#type), c.size)]],
            })
            .collect()
    }

    fn v(cells: &[(&str, u32)]) -> Vec<(String, u32)> {
        cells.iter().map(|&(t, s)| (t.to_string(), s)).collect()
    }

    #[test]
    fn height_adds_rows_blocks_and_spacing() {
        // 2 columns: [a b] [c] | [D] | [e]
        let c = sidebar(2, &[("a", 1), ("b", 1), ("c", 1), ("D", 2), ("e", 1)]);
        let (_, s) = sections(&c).unwrap();
        let m = Metrics {
            cell_padding: 6.0,
            cell_spacing: 4.0,
            section_spacing: 10.0,
        };
        let h = |c: &Cell| match c.r#type.as_str() {
            "a" => Some(50.0),
            "b" => Some(56.0),
            "D" => Some(200.0),
            _ => Some(56.0),
        };
        // (12 + 56 + 4 + 56) + 10 + 200 + 10 + (12 + 56)
        assert_eq!(height(&s, h, &m), Some(416.0));
        assert_eq!(height(&s, |c| (c.r#type != "e").then_some(1.0), &m), None);
    }

    #[test]
    fn packs_rows_like_the_original_quick_toggles() {
        // The original's default Android toggles in 5 columns: 2+2+1 | 1+2+2.
        let c = sidebar(
            5,
            &[
                ("network", 2),
                ("bluetooth", 2),
                ("idleInhibitor", 1),
                ("mic", 1),
                ("audio", 2),
                ("nightLight", 2),
            ],
        );
        let (cols, s) = sections(&c).unwrap();
        assert_eq!(cols, 5);
        assert_eq!(
            shape(&s),
            vec![vec![
                v(&[("network", 2), ("bluetooth", 2), ("idleInhibitor", 1)]),
                v(&[("mic", 1), ("audio", 2), ("nightLight", 2)]),
            ]]
        );
    }

    #[test]
    fn full_width_widgets_split_grid_sections() {
        let c = sidebar(
            4,
            &[
                ("sliders", 4),
                ("mic", 1),
                ("audio", 2),
                ("calendar", 4),
                ("notes", 9),
                ("darkMode", 0),
            ],
        );
        let (_, s) = sections(&c).unwrap();
        assert_eq!(
            shape(&s),
            vec![
                vec![v(&[("[sliders]", 4)])],
                vec![v(&[("mic", 1), ("audio", 2)])],
                vec![v(&[("[calendar]", 4)])],
                vec![v(&[("[notes]", 4)])],
                vec![v(&[("darkMode", 1)])],
            ]
        );
    }

    #[test]
    fn other_containers_have_no_sidebar_sections() {
        let mut c = sidebar(5, &[]);
        c.kind = ContainerKind::Desktop;
        assert!(sections(&c).is_none());
        assert_eq!(sections(&sidebar(0, &[("mic", 3)])).unwrap().0, 1);
    }
}
