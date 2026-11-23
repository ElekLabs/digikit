/// The two page dots in bottom-origin bitmap coordinates: page 0 is the upper dot, page 1 the lower one; other pages draw nothing.
pub fn dots(page: u32, mut rect: impl FnMut(i32, i32, i32, i32, i32)) {
    match page {
        0 => {
            rect(124, 31, 127, 34, -1);
            rect(125, 24, 126, 25, -1);
        }
        1 => {
            rect(125, 32, 126, 33, -1);
            rect(124, 23, 127, 26, -1);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
