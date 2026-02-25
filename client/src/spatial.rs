use crate::territory::ClientTerritoryMap;

const GRID_COLS: usize = 50;
const GRID_ROWS: usize = 50;

/// A flat 2D spatial grid over world space for O(1) territory hit-testing.
/// Rebuilt only when the territory map changes (SSE snapshot/update).
pub struct SpatialGrid {
    cells: Vec<Vec<usize>>,
    names: Vec<String>,
    lefts: Vec<i32>,
    rights: Vec<i32>,
    tops: Vec<i32>,
    bottoms: Vec<i32>,
    min_x: f64,
    min_y: f64,
    cell_w: f64,
    cell_h: f64,
}

impl SpatialGrid {
    pub fn build(territories: &ClientTerritoryMap) -> Self {
        if territories.is_empty() {
            return Self {
                cells: Vec::new(),
                names: Vec::new(),
                lefts: Vec::new(),
                rights: Vec::new(),
                tops: Vec::new(),
                bottoms: Vec::new(),
                min_x: 0.0,
                min_y: 0.0,
                cell_w: 1.0,
                cell_h: 1.0,
            };
        }

        // Compute world bounds
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for ct in territories.values() {
            let loc = &ct.territory.location;
            min_x = min_x.min(loc.left() as f64);
            min_y = min_y.min(loc.top() as f64);
            max_x = max_x.max(loc.right() as f64);
            max_y = max_y.max(loc.bottom() as f64);
        }

        // Add small padding to avoid edge issues
        min_x -= 1.0;
        min_y -= 1.0;
        max_x += 1.0;
        max_y += 1.0;

        let cell_w = (max_x - min_x) / GRID_COLS as f64;
        let cell_h = (max_y - min_y) / GRID_ROWS as f64;

        let mut cells = vec![Vec::new(); GRID_COLS * GRID_ROWS];
        let mut names = Vec::with_capacity(territories.len());
        let mut lefts = Vec::with_capacity(territories.len());
        let mut rights = Vec::with_capacity(territories.len());
        let mut tops = Vec::with_capacity(territories.len());
        let mut bottoms = Vec::with_capacity(territories.len());

        for (idx, (name, ct)) in territories.iter().enumerate() {
            let loc = &ct.territory.location;
            let l = loc.left();
            let r = loc.right();
            let t = loc.top();
            let b = loc.bottom();

            names.push(name.clone());
            lefts.push(l);
            rights.push(r);
            tops.push(t);
            bottoms.push(b);

            // Insert into all overlapping grid cells
            let col_start = ((l as f64 - min_x) / cell_w).floor().max(0.0) as usize;
            let col_end = ((r as f64 - min_x) / cell_w).ceil().min(GRID_COLS as f64) as usize;
            let row_start = ((t as f64 - min_y) / cell_h).floor().max(0.0) as usize;
            let row_end = ((b as f64 - min_y) / cell_h).ceil().min(GRID_ROWS as f64) as usize;

            for row in row_start..row_end {
                for col in col_start..col_end {
                    cells[row * GRID_COLS + col].push(idx);
                }
            }
        }

        Self {
            cells,
            names,
            lefts,
            rights,
            tops,
            bottoms,
            min_x,
            min_y,
            cell_w,
            cell_h,
        }
    }

    /// Returns the world-coordinate bounding box of all territories, or `None` if empty.
    pub fn world_bounds(&self) -> Option<(f64, f64, f64, f64)> {
        if self.cells.is_empty() {
            return None;
        }
        Some((
            self.min_x,
            self.min_y,
            self.min_x + self.cell_w * GRID_COLS as f64,
            self.min_y + self.cell_h * GRID_ROWS as f64,
        ))
    }

    /// Find the territory at a world coordinate. Returns `None` if no territory at that point.
    pub fn find_at(&self, wx: f64, wy: f64) -> Option<String> {
        if self.cells.is_empty() {
            return None;
        }

        let col = ((wx - self.min_x) / self.cell_w).floor() as isize;
        let row = ((wy - self.min_y) / self.cell_h).floor() as isize;

        if col < 0 || row < 0 || col >= GRID_COLS as isize || row >= GRID_ROWS as isize {
            return None;
        }

        let ix = wx as i32;
        let iy = wy as i32;

        let cell = &self.cells[row as usize * GRID_COLS + col as usize];
        for &idx in cell {
            if ix >= self.lefts[idx]
                && ix <= self.rights[idx]
                && iy >= self.tops[idx]
                && iy <= self.bottoms[idx]
            {
                return Some(self.names[idx].clone());
            }
        }
        None
    }
}
