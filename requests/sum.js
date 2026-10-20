function sumNumbers(array) {
    return (array || []).reduce(function (a, b) {
        return a + b;
    });
}

let numbers = Deno.core.ops.op_rs3_get("numbers").toString();
let arr = numbers.split(" ");
arr = arr.map(i => Number(i));
sumNumbers(arr);
