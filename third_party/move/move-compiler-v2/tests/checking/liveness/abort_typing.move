module 0x1234::M {
    fun test1(): u8 {
        abort(42)
    }

    fun test2(): u8 {
        abort(42);
    }

    fun test3(): u8 {
        abort 42;
    }

    fun test4(): u8 {
        abort 42
    }

    fun test5(): u8 {
	if (true) {
            abort 42;
	};
	42
    }
}