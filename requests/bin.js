function insertBinTree(t = {value: void 0, left: void 0, right: void 0}, n) {
    t.value !== void 0 ? t.value > n ? t.left = insertBinTree(t.left, n) : t.right = insertBinTree(t.right, n) : t.value = n;
    return t;
}

Deno.core.ops();
let numbers = Deno.core.opSync("rs3_get", "tree").toString();
let arr = numbers.split(" ");
arr = arr.map(i => Number(i));
let tree = arr.reduce(insertBinTree, void 0);
JSON.stringify(tree);