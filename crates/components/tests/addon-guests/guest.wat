;; A test add-on. The test fills in the parts in braces with its own ABI version, imports,
;; extra top-level parts and the body of `run`. `info` names the add-on, `alloc` hands out
;; room past everything else and grows the memory when it must.
(module
  {imports}
  (memory (export "memory") 2)
  (global $heap (mut i32) (i32.const 65536))
  (data (i32.const 1024) "{\"name\":\"test\",\"version\":\"1.0.0\",\"operations\":[\"export\",\"import\"]}")
  {extra}
  (func (export "addon_abi") (result i32) (i32.const {abi}))
  (func $alloc (export "alloc") (param $len i32) (result i32)
    (local $at i32)
    (local $end i32)
    (local.set $at (global.get $heap))
    (local.set $end (i32.add (local.get $at) (local.get $len)))
    (if (i32.gt_u (local.get $end) (i32.mul (memory.size) (i32.const 65536)))
      (then
        (if (i32.eq
              (memory.grow (i32.sub (i32.add (i32.shr_u (local.get $end) (i32.const 16)) (i32.const 1)) (memory.size)))
              (i32.const -1))
          (then unreachable))))
    (global.set $heap (local.get $end))
    (local.get $at))
  (func (export "info") (result i64)
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 66)))
  (func (export "run") (param $ptr i32) (param $len i32) (result i64)
    {run}))
