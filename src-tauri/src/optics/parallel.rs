// SPDX-License-Identifier: GPL-3.0-or-later
// Zero-dependency scoped thread parallelism for CPU optical workloads.

/// Returns the number of worker threads to use (capped reasonably for desktop responsiveness).
pub fn worker_count(items: usize, min_chunk: usize) -> usize {
    if items <= min_chunk {
        return 1;
    }
    let available = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let needed = items.div_ceil(min_chunk);
    available.min(needed).clamp(1, 32)
}

/// Maps `0..len` in parallel using scoped threads, preserving output order.
pub fn par_map_index<T, F>(len: usize, min_chunk: usize, f: F) -> Vec<T>
where
    T: Send,
    F: Fn(usize) -> T + Sync,
{
    let workers = worker_count(len, min_chunk);
    if workers <= 1 {
        return (0..len).map(f).collect();
    }
    let chunk = len.div_ceil(workers);
    let mut chunks: Vec<Vec<T>> = Vec::with_capacity(workers);
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for worker in 0..workers {
            let start = worker * chunk;
            if start >= len {
                break;
            }
            let end = (start + chunk).min(len);
            let func = &f;
            handles.push(scope.spawn(move || (start..end).map(func).collect::<Vec<T>>()));
        }
        for handle in handles {
            chunks.push(handle.join().expect("parallel worker panicked"));
        }
    });
    let mut out = Vec::with_capacity(len);
    for mut part in chunks {
        out.append(&mut part);
    }
    out
}

/// Maps a slice in parallel using scoped threads, preserving output order.
pub fn par_map<I, T, F>(items: &[I], min_chunk: usize, f: F) -> Vec<T>
where
    I: Sync,
    T: Send,
    F: Fn(&I) -> T + Sync,
{
    par_map_index(items.len(), min_chunk, |i| f(&items[i]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn par_map_matches_sequential_order() {
        let input: Vec<usize> = (0..257).collect();
        let out = par_map(&input, 16, |x| x * x + 3);
        let expected: Vec<usize> = input.iter().map(|x| x * x + 3).collect();
        assert_eq!(out, expected);
    }
}
