function factorial(n) {
    if (n === 0) {
        return 1;
    }
    return n * factorial(n - 1);
}

Deno.core.ops();
let num = Number(Deno.core.opSync("rs3_get", "num").toString());
factorial(num);