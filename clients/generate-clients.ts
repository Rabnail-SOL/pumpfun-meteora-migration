import {
  AnchorIdl,
  rootNodeFromAnchorWithoutDefaultVisitor,
} from "@codama/nodes-from-anchor";
import {
  renderJavaScriptUmiVisitor,
  renderJavaScriptVisitor,
  renderRustVisitor,
} from "@codama/renderers";
import { visit } from "@codama/visitors-core";
import anchorIdl from "../target/idl/coinfun.json"; // Note: if you initiated your project with a different name, you may need to change this path

async function generateClients() {
  const node = rootNodeFromAnchorWithoutDefaultVisitor(anchorIdl as AnchorIdl);

  const clients = [
    {
      type: "JS",
      dir: "clients/generated/js/src",
      renderVisitor: renderJavaScriptVisitor,
    },
    {
      type: "Umi",
      dir: "clients/generated/umi/src",
      renderVisitor: renderJavaScriptUmiVisitor,
    },
    {
      type: "Rust",
      dir: "clients/generated/rust/src",
      renderVisitor: renderRustVisitor,
    },
  ];

  for (const client of clients) {
    try {
      await visit(node, await client.renderVisitor(client.dir));
      console.log(
        `✅ Successfully generated ${client.type} client for directory: ${client.dir}!`
      );
    } catch (e) {
      console.error(`Error in ${client.renderVisitor.name}:`, e);
      throw e;
    }
  }
}

generateClients();
