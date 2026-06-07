import type { ReactNode } from "react";

export function CenteredStatus({
  children,
  title,
  detail,
}: {
  children?: ReactNode;
  title: string;
  detail: string;
}) {
  return (
    <section className="centered-status">
      <h1>{title}</h1>
      <p>{detail}</p>
      {children}
    </section>
  );
}
