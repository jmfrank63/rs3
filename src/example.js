function print(value) {
    Deno.core.print(value.toString()+"\n");
}

let res = Deno.core.ops.op_rs3_list();
print(res);
