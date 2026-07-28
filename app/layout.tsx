import type { Metadata } from "next";
import { headers } from "next/headers";
import "./globals.css";

export async function generateMetadata(): Promise<Metadata> {
  const incoming = await headers();
  const host =
    incoming.get("x-forwarded-host") ??
    incoming.get("host") ??
    "localhost:3000";
  const protocol =
    incoming.get("x-forwarded-proto") ??
    (host.startsWith("localhost") || host.startsWith("127.0.0.1")
      ? "http"
      : "https");
  const metadataBase = new URL(`${protocol}://${host}`);
  const title = "Kiln — Local-first agent workbench";
  const description =
    "Direct OpenAI, Anthropic, and local coding agents from one calm, inspectable workspace.";
  const socialCard = "/kiln-social-card-1200x630.png";

  return {
    metadataBase,
    title: {
      default: title,
      template: "%s · Kiln",
    },
    description,
    applicationName: "Kiln",
    keywords: [
      "coding agent",
      "local AI",
      "OpenAI",
      "Anthropic",
      "Tauri",
      "Rust",
    ],
    openGraph: {
      type: "website",
      url: "/",
      title,
      description,
      siteName: "Kiln",
      images: [
        {
          url: socialCard,
          width: 1200,
          height: 630,
          alt: "Kiln — one calm surface for every coding agent",
        },
      ],
    },
    twitter: {
      card: "summary_large_image",
      title,
      description,
      images: [socialCard],
    },
  };
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
