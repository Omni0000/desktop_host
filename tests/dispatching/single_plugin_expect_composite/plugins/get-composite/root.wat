(component
  (core module $m
    (memory (export "memory") 1)
    (func $get_data (export "get-composite") (result i32)
      i32.const 0
      i32.const 42
      i32.store
      i32.const 4
      i32.const 24
      i32.store
      i32.const 0
    )
  )
  (core instance $i (instantiate $m))
  (func $f (export "get-composite") (result (tuple u32 u32)) (canon lift (core func $i "get-composite") (memory $i "memory")))
  (instance $inst
    (export "get-composite" (func $f))
  )
  (export "test:composite/root" (instance $inst))
)
