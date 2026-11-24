;; The `run` of a guest that gives its files back: it skips `.op` and `.args.json`, which the
;; app always sends first, copies the rest and adds a `.report.json` of `{"notices":[]}`.
;; The report's entry is a data segment at 2048 that the test places.
(local $at i32)
(local $skip i32)
(local $rest i32)
(local $out i32)
(local.set $at (i32.add (local.get $ptr) (i32.const 8)))
(local.set $skip (i32.const 2))
(loop $skipping
  (local.set $at (i32.add (local.get $at) (i32.add (i32.const 4) (i32.load (local.get $at)))))
  (local.set $at (i32.add (local.get $at) (i32.add (i32.const 4) (i32.load (local.get $at)))))
  (local.set $skip (i32.sub (local.get $skip) (i32.const 1)))
  (br_if $skipping (local.get $skip)))
(local.set $rest (i32.sub (i32.add (local.get $ptr) (local.get $len)) (local.get $at)))
(local.set $out (call $alloc (i32.add (local.get $rest) (i32.const 42))))
;; "SPB1", and one file fewer: two left out, the report put in.
(i32.store (local.get $out) (i32.load (local.get $ptr)))
(i32.store (i32.add (local.get $out) (i32.const 4)) (i32.sub (i32.load (i32.add (local.get $ptr) (i32.const 4))) (i32.const 1)))
(memory.copy (i32.add (local.get $out) (i32.const 8)) (local.get $at) (local.get $rest))
(memory.copy (i32.add (i32.add (local.get $out) (i32.const 8)) (local.get $rest)) (i32.const 2048) (i32.const 34))
(i64.or
  (i64.shl (i64.extend_i32_u (local.get $out)) (i64.const 32))
  (i64.extend_i32_u (i32.add (local.get $rest) (i32.const 42))))
