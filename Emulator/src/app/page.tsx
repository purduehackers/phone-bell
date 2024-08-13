"use client";

import { PhoneType, usePhoneBell } from "@/components/server-provider";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Bell, BellRing, House, Phone, Trees } from "lucide-react";

export default function Home() {
	const [
		ringing,
		hooked,
		dialedNumber,

		connectionState,

		setHookedState,
		setPhoneType,
		dialNumber,
	] = usePhoneBell();

	return (
		<main className="bg-background flex justify-center items-center">
			<div className="flex shrink flex-col">
				<div className="grid grid-rows-4 grid-cols-3 gap-2">
					<div className="col-span-3 flex flex-col justify-center items-center text-center p-2">
						{["Connecting", "Connected", "Error"][connectionState]}
					</div>
					<div className="col-span-3 flex flex-row justify-center items-center text-center p-2 gap-3">
						<Trees />
						<Switch
							className="data-[state=checked]:bg-primary data-[state=unchecked]:bg-primary"
							onCheckedChange={(e) =>
								setPhoneType(
									e ? PhoneType.Inside : PhoneType.Outside
								)
							}
						/>
						<House />
					</div>
					<div className="col-span-3 flex flex-col justify-center items-center text-center p-2">
						{ringing ? <BellRing /> : <Bell />}
					</div>
					<div className="col-span-3 flex flex-col justify-center items-center text-left p-2">
						<p className="m-auto w-full font-mono">{`> ${dialedNumber}`}</p>
					</div>
					<Button variant="default" onClick={() => dialNumber("1")}>
						1
					</Button>
					<Button variant="default" onClick={() => dialNumber("2")}>
						2
					</Button>
					<Button variant="default" onClick={() => dialNumber("3")}>
						3
					</Button>
					<Button variant="default" onClick={() => dialNumber("4")}>
						4
					</Button>
					<Button variant="default" onClick={() => dialNumber("5")}>
						5
					</Button>
					<Button variant="default" onClick={() => dialNumber("6")}>
						6
					</Button>
					<Button variant="default" onClick={() => dialNumber("7")}>
						7
					</Button>
					<Button variant="default" onClick={() => dialNumber("8")}>
						8
					</Button>
					<Button variant="default" onClick={() => dialNumber("9")}>
						9
					</Button>
					<Button variant="default" onClick={() => dialNumber("0")}>
						0
					</Button>
					<Button
						className="col-span-2"
						variant="default"
						onClick={() => setHookedState(!hooked)}
					>
						<Phone
							className="transition-all"
							style={{
								rotate: `${hooked ? "135" : "0"}deg`,
							}}
						/>
					</Button>
				</div>
			</div>
		</main>
	);
}
