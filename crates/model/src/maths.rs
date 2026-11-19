//! The little arithmetic of placing a mesh: 4 by 4 matrices, as glTF writes them, by columns.

pub(crate) type Matrix = [f32; 16];

pub(crate) const IDENTITY: Matrix = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

pub(crate) fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            out[column * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[column * 4 + k]).sum();
        }
    }
    out
}

/// A translation, then a rotation by the unit quaternion `[x, y, z, w]`, then a scale.
pub(crate) fn from_parts(translation: [f32; 3], rotation: [f32; 4], scale: [f32; 3]) -> Matrix {
    let [x, y, z, w] = rotation;
    let rotation = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
    ];
    let mut out = IDENTITY;
    for column in 0..3 {
        for row in 0..3 {
            out[column * 4 + row] = rotation[column * 3 + row] * scale[column];
        }
    }
    out[12..15].copy_from_slice(&translation);
    out
}

pub(crate) fn place(m: &Matrix, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|row| m[row] * p[0] + m[4 + row] * p[1] + m[8 + row] * p[2] + m[12 + row])
}

/// The matrix normals are turned by: the inverse of the turning part, transposed. Its
/// determinant's sign says whether triangles turn the other way once placed.
pub(crate) fn normal_matrix(m: &Matrix) -> ([[f32; 3]; 3], f32) {
    let a = |row: usize, column: usize| m[column * 4 + row];
    let cofactor = |row: usize, column: usize| {
        let (r0, r1) = ((row + 1) % 3, (row + 2) % 3);
        let (c0, c1) = ((column + 1) % 3, (column + 2) % 3);
        a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0)
    };
    let determinant = (0..3).map(|column| a(0, column) * cofactor(0, column)).sum::<f32>();
    // The inverse transposed is the cofactors over the determinant; only the direction matters,
    // as normals are made of length one again, so only its sign is kept.
    let sign = if determinant < 0.0 { -1.0 } else { 1.0 };
    let normal = [0, 1, 2].map(|row| [0, 1, 2].map(|column| cofactor(row, column) * sign));
    (normal, determinant)
}

pub(crate) fn turn(n: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|row| n[row][0] * v[0] + n[row][1] * v[1] + n[row][2] * v[2])
}

/// `v` made of length one, or `None` if it has no length to speak of.
pub(crate) fn unit(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (length.is_finite() && length > 1e-20).then(|| v.map(|x| x / length))
}

pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub(crate) fn minus(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
