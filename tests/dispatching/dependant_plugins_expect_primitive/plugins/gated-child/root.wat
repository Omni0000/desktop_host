(component
	(import "test:gate/gate" (instance $gate
		(export "wait" (func $wait))
	))
	(core func $wait (canon lower (func $gate "wait")))
	(core instance $gate-core
		(export "wait" (func $wait))
	)
	(core module $m
		(import "gate" "wait" (func $wait))
		(func $get_value (export "get-value") (result i32)
			call $wait
			i32.const 42
		)
	)
	(core instance $i (instantiate $m
		(with "gate" (instance $gate-core))
	))
	(func $f (export "get-value") (result u32) (canon lift (core func $i "get-value")))
	(instance $inst
		(export "get-value" (func $f))
	)
	(export "test:child/root" (instance $inst))
)
