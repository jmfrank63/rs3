Deno.core.ops();
let p = "1111";
Deno.core.opSync("rs3_insert", "{\"res\":\"" + p + "\"}");
