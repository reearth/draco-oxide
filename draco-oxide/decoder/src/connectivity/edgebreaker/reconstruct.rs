//! Corner-table reconstruction (spirale-reversi). Replays the symbol stream over an
//! active-corner stack (C/R/L/S/E), merging vertices on S, closing start-face fans,
//! and compacting isolated vertices, producing a `CornerTable` plus the
//! corner-to-point map.
