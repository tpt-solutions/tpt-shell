// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// The till page. The price list is fixed here; the host checks every item before it prints.
const PRODUCTS = [
  { name: "Flat white", cents: 450 },
  { name: "Long black", cents: 380 },
  { name: "Muffin", cents: 520 },
  { name: "Sandwich", cents: 890 },
];

const productsEl = document.getElementById("products");
const cartEl = document.getElementById("cart");
const totalEl = document.getElementById("total");
const printBtn = document.getElementById("print");
const statusEl = document.getElementById("status");

/** @type {{ name: string, cents: number }[]} */
let cart = [];

function money(cents) {
  return (cents / 100).toFixed(2);
}

function render() {
  cartEl.replaceChildren(
    ...cart.map((item) => {
      const li = document.createElement("li");
      li.textContent = `${item.name} ${money(item.cents)}`;
      return li;
    })
  );
  const total = cart.reduce((sum, item) => sum + item.cents, 0);
  totalEl.textContent = money(total);
  printBtn.disabled = cart.length === 0 || !window.tpt;
}

productsEl.replaceChildren(
  ...PRODUCTS.map((product) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = `${product.name} ${money(product.cents)}`;
    btn.addEventListener("click", () => {
      cart.push({ name: product.name, cents: product.cents });
      statusEl.textContent = "";
      render();
    });
    return btn;
  })
);

printBtn.addEventListener("click", () => {
  printBtn.disabled = true;
  statusEl.textContent = "Printing...";
  window.tpt
    .invoke("receipt.print", { items: cart })
    .then((reply) => {
      statusEl.textContent = `Printed ${reply.printed} items, total ${reply.total}`;
      cart = [];
      render();
    })
    .catch((error) => {
      statusEl.textContent = `${error.code}: ${error.message}`;
      render();
    });
});

if (!window.tpt) {
  statusEl.textContent = "Open this page in the app window to print.";
}
render();
