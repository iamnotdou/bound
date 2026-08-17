"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/utils";
import { ConnectWalletButton } from "./ConnectWalletButton";

const LINKS = [
  { href: "/", label: "Home" },
  { href: "/dashboard", label: "Verify" },
  { href: "/chat", label: "Agent" },
  { href: "/control", label: "Control" },
  { href: "/auditor", label: "Auditor" },
];

export function Nav() {
  const pathname = usePathname();
  return (
    <header className="sticky top-0 z-40 border-b bg-background/80 backdrop-blur supports-[backdrop-filter]:bg-background/60">
      <div className="mx-auto flex h-14 max-w-6xl items-center justify-between gap-4 px-4">
        <div className="flex items-center gap-6">
          <Link href="/" className="font-semibold tracking-tight">
            Bound
          </Link>
          <nav className="hidden items-center gap-1 sm:flex">
            {LINKS.map((l) => {
              const active = l.href === "/" ? pathname === "/" : pathname.startsWith(l.href);
              return (
                <Link
                  key={l.href}
                  href={l.href}
                  className={cn(
                    "rounded-md px-3 py-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground",
                    active && "bg-muted text-foreground",
                  )}
                >
                  {l.label}
                </Link>
              );
            })}
          </nav>
        </div>
        <ConnectWalletButton />
      </div>
    </header>
  );
}
