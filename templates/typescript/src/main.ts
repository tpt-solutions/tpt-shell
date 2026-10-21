import { commands } from "./commands.js";

const out = document.getElementById("out");

commands
  .greet({ name: "world" })
  .then((reply) => {
    if (out) out.textContent = String(reply);
  })
  .catch((error: { code: string; message: string }) => {
    if (out) out.textContent = `${error.code}: ${error.message}`;
  });
