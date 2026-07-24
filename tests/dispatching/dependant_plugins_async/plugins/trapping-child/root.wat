(component
	(core module $m
		(func $get_value (export "get-value") (result i32)
			unreachable
		)
	)
	(core instance $i (instantiate $m))
	(func $f (export "get-value") async (result u32)
		(canon lift (core func $i "get-value"))
	)
	(instance $inst
		(export "get-value" (func $f))
	)
	(export "test:async-child/root" (instance $inst))
)
