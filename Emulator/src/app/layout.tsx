import type { Metadata } from "next";
import { Inter } from "next/font/google";
import "./globals.css";
import { ThemeProvider } from "@/components/theme-provider";
import { PhoneBellProvider } from "@/components/server-provider";

const inter = Inter({ subsets: ["latin"] });

export const metadata: Metadata = {
	title: "Phone Bell Emulator",
	description: "i'm so tired",
};

export default function RootLayout({
	children,
}: Readonly<{
	children: React.ReactNode;
}>) {
	return (
		<html lang="en" suppressHydrationWarning>
			<body className={inter.className}>
				<ThemeProvider
					attribute="class"
					defaultTheme="dark"
					enableSystem
					disableTransitionOnChange
				>
					<PhoneBellProvider>{children}</PhoneBellProvider>
				</ThemeProvider>
			</body>
		</html>
	);
}
