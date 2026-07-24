(component
	(core module $m
		(memory (export "memory") 1)
		(global $sequence (mut i32) (i32.const 0))
		(func $run (export "run") (param $value i32) (result i32)
			global.get $sequence
			i32.const 1
			i32.add
			global.set $sequence
			i32.const 0
			global.get $sequence
			i32.store
			i32.const 4
			local.get $value
			i32.store
			i32.const 0
		)
	)
	(core instance $i (instantiate $m))
	(func $run (export "run") async (param "value" u32) (result (tuple u32 u32))
		(canon lift (core func $i "run") (memory $i "memory"))
	)
	(instance $root (export "run" (func $run)))
	(export "test:dispatch-serialization/root" (instance $root))
)
