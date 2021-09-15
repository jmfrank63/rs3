function sumNumbers(array) {
    return (array || []).reduce(function (a, b) {
        return a + b;
    });
}

Deno.core.ops();
let numbers = Deno.core.opSync("rs3_get", "numbers").toString();
let arr = numbers.split(" ");
arr = arr.map(i => Number(i));
sumNumbers(arr);
