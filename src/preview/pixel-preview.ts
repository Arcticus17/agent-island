import { mount } from "svelte";

import PixelPreview from "./PixelPreview.svelte";

const target = document.getElementById("preview");
if (target) mount(PixelPreview, { target });
