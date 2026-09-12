// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

const findToolForm = (toolName) => {
  for (const form of document.querySelectorAll("form[toolname]")) {
    if (form.getAttribute("toolname") === toolName) return form;
  }
  return null;
};

const announce = (form, message) => {
  const status = form?.querySelector("[data-agent-state]");
  if (status) status.textContent = message;
};

const inputSchemaFor = (form) => {
  const properties = {};
  const required = [];

  for (const control of form.elements) {
    if (
      !(
        control instanceof HTMLInputElement ||
        control instanceof HTMLSelectElement ||
        control instanceof HTMLTextAreaElement
      ) ||
      !control.name
    ) {
      continue;
    }

    const property = { type: "string" };
    const description = control.getAttribute("toolparamdescription");
    if (description) property.description = description;

    if (control instanceof HTMLSelectElement) {
      property.enum = Array.from(control.options, ({ value }) => value);
    } else {
      if (control.minLength > 0) property.minLength = control.minLength;
      if (control.maxLength >= 0) property.maxLength = control.maxLength;
    }

    properties[control.name] = property;
    if (control.required) required.push(control.name);
  }

  return {
    type: "object",
    properties,
    required,
    additionalProperties: false,
  };
};

const populateNativeForm = (form, input) => {
  for (const [name, value] of Object.entries(input)) {
    const control = form.elements.namedItem(name);
    if (
      !(
        control instanceof HTMLInputElement ||
        control instanceof HTMLSelectElement ||
        control instanceof HTMLTextAreaElement
      )
    ) {
      throw new Error(`Unknown form control ${name}`);
    }

    control.value = String(value);
    control.dispatchEvent(new Event("input", { bubbles: true }));
    control.dispatchEvent(new Event("change", { bubbles: true }));
  }

  form.dataset.agentActive = "true";
  form.focus();
  announce(form, "Agent populated the form — review and confirm");
};

const registerImperativeAdapter = async (form) => {
  if (
    !("modelContext" in document) ||
    typeof document.modelContext.registerTool !== "function"
  ) {
    return;
  }

  const declarativeName = form.getAttribute("toolname");
  const description = form.getAttribute("tooldescription");
  if (!declarativeName || !description) return;

  // Some agent clients currently discover imperative registrations but omit
  // tools synthesized from declarative forms. Give those clients a distinct
  // adapter. Prefer delegating to the browser-owned declarative tool; fall
  // back to the same visible native-form operation when that tool is absent.
  const adapterName = `configure_${declarativeName.replace(/^create_/, "")}`;

  await document.modelContext.registerTool({
    name: adapterName,
    description: `${description}. Leaves the populated form visible for user review and confirmation.`,
    inputSchema: inputSchemaFor(form),
    annotations: {
      readOnlyHint: false,
      untrustedContentHint: false,
    },
    execute: async (input) => {
      const tools = await document.modelContext.getTools();
      const declarativeTool = tools.find(
        (tool) => tool.name === declarativeName,
      );

      if (declarativeTool) {
        await document.modelContext.executeTool(
          declarativeTool,
          JSON.stringify(input),
        );
        return `Configured ${declarativeName} through declarative WebMCP; waiting for user confirmation`;
      }

      populateNativeForm(form, input);
      return `Configured ${declarativeName} through its imperative adapter; waiting for user confirmation`;
    },
  });
};

// Declarative WebMCP populates native controls before this event. The current
// draft does not promise that those writes emit ordinary `input`/`change`
// events, so replay them inside this form to reconcile RSJS `bind_value`
// signals with the browser-owned control values.
window.addEventListener("toolactivated", (event) => {
  const form = findToolForm(event.toolName);
  if (!form) return;

  form.dataset.agentActive = "true";
  announce(form, "Agent populated the form — review and confirm");

  for (const control of form.elements) {
    if (!(control instanceof HTMLElement) || !control.hasAttribute("name")) {
      continue;
    }
    control.dispatchEvent(new Event("input", { bubbles: true }));
    control.dispatchEvent(new Event("change", { bubbles: true }));
  }
});

window.addEventListener("toolcancel", (event) => {
  const form = findToolForm(event.toolName);
  if (!form) return;

  delete form.dataset.agentActive;
  announce(form, "Agent operation cancelled");
});

for (const form of document.querySelectorAll("form[toolname]")) {
  announce(
    form,
    "modelContext" in document
      ? "WebMCP ready — waiting for a person or agent"
      : "Native form ready — WebMCP is not enabled in this browser",
  );

  registerImperativeAdapter(form).catch((error) => {
    console.error("Unable to register WebMCP imperative adapter", error);
  });
}
