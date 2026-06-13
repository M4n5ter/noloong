import { describe, expect, it } from "vitest";
import { pathToFileUri, submissionContentBlocks } from "./attachments";

describe("pathToFileUri", () => {
  it("preserves existing file URIs", () => {
    expect(pathToFileUri("file:///tmp/reference.png")).toBe("file:///tmp/reference.png");
  });

  it("encodes POSIX paths as file URIs", () => {
    expect(pathToFileUri("/tmp/reference image.png")).toBe("file:///tmp/reference%20image.png");
  });

});

describe("submissionContentBlocks", () => {
  it("uses normalized file URIs for attachments", () => {
    expect(
      submissionContentBlocks({
        text: "",
        attachments: [
          {
            path: "/tmp/reference.png",
            name: "reference.png",
            mimeType: "image/png",
            kind: "image",
          },
        ],
      }),
    ).toMatchObject([
      {
        type: "media",
        media: {
          source: { type: "uri", uri: "file:///tmp/reference.png" },
        },
      },
    ]);
  });
});
