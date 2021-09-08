function print(value) {
    Deno.core.print(value.toString()+"\n");
}

Deno.core.ops();
let res = Deno.core.opSync('op_list', '');
print(res);
