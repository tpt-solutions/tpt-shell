// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// The till page. It prints to the virtual printer the host granted, and shows what that printer
// kept: the receipt preview and the audit lines the device hub wrote.
const PRINTER = "usb:0416:5011";
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
const previewEl = document.getElementById("preview");

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
  totalEl.textContent = money(cart.reduce((sum, item) => sum + item.cents, 0));
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
    .invoke("receipt.print", { device: PRINTER, items: cart })
    .then((reply) => {
      statusEl.textContent = `Printed ${reply.printed} items, total ${reply.total}`;
      previewEl.textContent = `${reply.preview}\n${reply.audit.join("\n")}`;
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
