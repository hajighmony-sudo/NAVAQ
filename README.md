<div dir="rtl">

# NAVAQ — پخش‌کنندهٔ موسیقی شیشه‌ای با هستهٔ Rust

NAVAQ یک برنامهٔ موسیقی اندروید است با رابط کاربری شیشه‌ای و کاملاً قابل شخصی‌سازی (با ظاهر الهام‌گرفته از Samsung Music)، فارسی و راست‌به‌چپ. پخش و صدا با Media3 (ExoPlayer) انجام می‌شود و منطق مشترک (صف، اکولایزر، DSP، ساخت آهنگ، تحلیل ضرب، مدل اتاق، سلیقهٔ موسیقی) در هستهٔ Rust نوشته شده و از طریق JNI به اندروید وصل است.

> وضعیت راستی‌آزمایی: هستهٔ Rust تست واحد دارد (۲۳۰ تست). لایهٔ Kotlin/JNI اندروید در محیط توسعه فقط بررسی نحوی شده؛ ساخت APK و تست روی گوشی واقعی را باید CI یا شما انجام دهید.

## فهرست قابلیت‌ها

1. [کتابخانه، جستجو و پلی‌لیست‌ها](#1-کتابخانه-جستجو-و-پلی‌لیست‌ها)
2. [پخش‌کنندهٔ تمام‌صفحه](#2-پخش‌کنندهٔ-تمام‌صفحه)
3. [تم و رابط شیشه‌ای](#3-تم-و-رابط-شیشه‌ای)
4. [صدا: اکولایزر و صدای فضایی شخصی](#4-صدا-اکولایزر-و-صدای-فضایی-شخصی)
5. [لرزش همراه ضرب (Haptics)](#5-لرزش-همراه-ضرب-haptics)
6. [ساخت آهنگ (متن به ملودی و آهنگ بی‌کلام)](#6-ساخت-آهنگ)
7. [Music DNA (مقایسهٔ سلیقه)](#7-music-dna)
8. [گوش دادن مشترک](#8-گوش-دادن-مشترک)
9. [NAVAQ Downloader](#9-navaq-downloader)
10. [کشف موسیقی (Discover)](#10-کشف-موسیقی-discover)
11. [دستیار صوتی AVA](#11-دستیار-صوتی-ava)
12. [کنترل‌های سیستم و اعلان‌ها](#12-کنترل‌های-سیستم-و-اعلان‌ها)
13. [پشتیبان و بازنشانی](#13-پشتیبان-و-بازنشانی)
14. [منوی آهنگ (نگه‌داشتن انگشت)](#14-منوی-آهنگ)
15. [متن همگام آهنگ (LRC)](#15-متن-همگام-آهنگ-lrc)
16. [آمار گوش‌دادن، «اخیراً» و «پرشنوی‌ها»](#16-آمار-گوش‌دادن)
17. [ادامه پخش، محو‌شدن تایمر خواب و رد کردن سکوت](#17-ادامه-پخش-محو‌شدن-تایمر-خواب-و-رد-کردن-سکوت)

برای همهٔ بخش‌ها: دکمهٔ ⚙ (تنظیمات) بالای صفحهٔ اصلی را بزنید؛ در تنظیمات نوار جستجو هم هست و بین همهٔ گزینه‌ها جستجو می‌کند.

---

### 1. کتابخانه، جستجو و پلی‌لیست‌ها

**چیست:** موسیقی‌های گوشی با چند تب: آهنگ‌ها، آلبوم‌ها، هنرمندان (در تنظیمات قابل خاموش‌کردن)، علاقه‌مندی‌ها و پلی‌لیست‌ها. دانلودهای NAVAQ و آهنگ‌های ساخته‌شده هم خودکار در کتابخانه می‌آیند.

**نحوه استفاده**
- بار اول اجازهٔ دسترسی به موسیقی را بدهید. بعد از آن کتابخانه خودکار پر می‌شود (تنظیمات › کتابخانه › «اسکن دوباره» برای بروزرسانی).
- روی یک آهنگ بزنید تا همان فهرست به‌عنوان صف پخش شود. روی قلب بزنید تا به علاقه‌مندی‌ها برود.
- ذره‌بین بالای صفحه: جستجو در عنوان، هنرمند و آلبوم؛ حروف عربی/فارسی و ارقام یکسان‌سازی می‌شوند (مثلاً «ك» و «ک»).
- تب پلی‌لیست‌ها: «＋ پلی‌لیست جدید» می‌سازد؛ روی پلی‌لیست بزنید تا آهنگ‌هایش را ببینید و «حذف» بزنید تا پاک شود. برای افزودن آهنگ، در پخش‌کنندهٔ تمام‌صفحه دکمهٔ کتابخانه را بزنید و پلی‌لیست را انتخاب کنید.
- تنظیمات › کتابخانه: مرتب‌سازی (نام، هنرمند، مدت) و «حداقل طول آهنگ» برای حذف صداهای کوتاه (مثل زنگ‌ها).

**کارکرد:** فهرست از MediaStore خوانده می‌شود، علاقه‌مندی‌ها و پلی‌لیست‌ها در حافظهٔ خصوصی برنامه می‌مانند.

### 2. پخش‌کنندهٔ تمام‌صفحه

**نحوه استفاده:** روی مینی‌پلیر پایین صفحه بزنید.
- پخش/توقف، قبلی/بعدی، جلو/عقب با نوار زمان.
- درهم (shuffle) و تکرار (خاموش / همه / یک آهنگ).
- سرعت پخش (۰٫۵× تا ۲×) و تایمر خواب (۱۵ تا ۶۰ دقیقه یا «پایان این آهنگ»).
- صف پخش، علاقه‌مندی، افزودن به پلی‌لیست.
- رنگ زمینه با کاور آهنگ هماهنگ می‌شود و نوار موج زیر کاور می‌تواند نشان داده شود.

**کارکرد:** Media3 MediaSession؛ در قفل‌صفحه، اعلان، هدفون و بلوتوث هم کنترل می‌شود و با Music Island / Now Bar دستگاه‌های سازگار هماهنگ است (ظاهر دقیق به سازندهٔ گوشی بستگی دارد).

### 3. تم و رابط شیشه‌ای

**نحوه استفاده:** تنظیمات › «تم و رنگ»، «شیشه و جلوه‌ها»، «چیدمان و رابط».
- قالب‌های آماده (شفق، شکوفه، نیمه‌شب، مینیمال …).
- حالت روشن/تاریک/خودکار، رنگ تأکید دلخواه (کد hex)، تصویر زمینهٔ متحرک.
- شیشه: میزان محو‌شدگی، شفافیت، گردی گوشه‌ها، درخشش لبه‌ها.
- حرکت: انیمیشن‌ها، نوار موج، چرخش کاور دایره‌ای.
- اندازهٔ متن، تراکم فهرست، شکل کاور (مربع/گرد/دایره)، نمایش فهرست یا شبکه، نمایش مدت، نوار ناوبری شناور یا چسبیده، تب هنرمندان.

**کارکرد:** مدل `UiSettings` در Rust (`ui_settings.rs`) و آینهٔ Kotlin آن تنظیمات را اعتبارسنجی و پاک‌سازی می‌کنند؛ تغییرات فوری اعمال و ذخیره می‌شوند. محو‌شدگی واقعی روی اندروید ۱۲ به بالا با `RenderEffect` است.

### 4. صدا: اکولایزر و صدای فضایی شخصی

**نحوه استفاده:** تنظیمات › «صدا و اکولایزر».
- اکولایزر ۱۰ باندی با قالب‌های آماده (باس، وکال، راک …) و تنظیم دستی هر باند.
- «صدای فضایی اتاق»: عرض، طول و ارتفاع اتاق، میزان مبلمان و پرده و شدت طنین را وارد کنید؛ برنامه زمان واکنش اتاق (RT60) را نشان می‌دهد.

**کارکرد:** صدا پیش از خروجی از زنجیرهٔ DSP هستهٔ Rust عبور می‌کند (بهره، اکولایزر، پژواک/طنین، سقف قله). مدل اتاق (فرمول Sabine، جذب سطوح و مبلمان) در هسته محاسبه و به‌صورت یک طنین ساده روی صدا اعمال می‌شود. این یک رندر دوگوشی HRTF کامل نیست.

### 5. لرزش همراه ضرب (Haptics)

**نحوه استفاده:** تنظیمات › «پخش» › «لرزش همراه ضرب» را روشن کنید (پیش‌فرض خاموش). «تحلیل پیشرفته ضرب» طبل/باس، اسنیر و های‌هت را جدا تشخیص می‌دهد و الگوی لرزش متفاوت می‌دهد.

**کارکرد:** `HapticAnalyzer` هسته روی همان PCMِ در حال پخش سه باند فرکانسی را بررسی می‌کند؛ بدون ضبط میکروفون. حالت ساده فقط شروع انرژی (onset) را تشخیص می‌دهد.

### 6. ساخت آهنگ

**نحوه استفاده:** تنظیمات › «ساخت آهنگ».
- متن آهنگ را بنویسید (فارسی هم پشتیبانی می‌شود) یا «آهنگ بی‌کلام» را روشن کنید و تعداد بندها را بدهید.
- سبک (پاپ، غمگین، راک، لوفای، جاز، ایرانی، سینمایی)، گام و سرعت (BPM) را انتخاب کنید.
- «ساخت و پخش»: ملودی، هارمونی، باس و درام ساخته، به‌صورت WAV و MIDI ذخیره و در کتابخانه (آلبوم «ساخته‌شده») پخش می‌شود.

**کارکرد:** `melody.rs` و `composition.rs` به‌صورت آفلاین و قطعی (برای یک متن و seed یکسان، خروجی یکسان) آهنگ می‌سازند. این مولد قاعده‌محور است، نه مدل هوش مصنوعی مولد.

### 7. Music DNA

**چیست:** مقایسهٔ سلیقهٔ موسیقی با دوستان، بدون سرور و بدون ارسال اطلاعات.

**نحوه استفاده:** تنظیمات › «Music DNA».
1. «کپی کد DNA من» را بزنید و کد را برای دوستتان بفرستید.
2. کد دوست را در کادر بچسبانید و «مقایسه سلیقه» را بزنید: درصد شباهت و هنرمندان مشترک نشان داده می‌شود.
3. «ساخت پلی‌لیست مشترک»: از آهنگ‌های هنرمندان مشترک در کتابخانهٔ خودتان یک پلی‌لیست می‌سازد و پخش می‌کند.

**کارکرد:** کد فقط شامل نام هنرمندان و وزن آن‌هاست (علاقه‌مندی‌ها وزن بیشتر)، به‌صورت base64. مقایسه با شباهت کسینوسی در هستهٔ Rust روی خود گوشی انجام می‌شود.

### 8. گوش دادن مشترک

**چیست:** مثل وصل‌کردن گوشی به تلویزیون در یوتیوب: یک نفر اتاق می‌سازد، بقیه با کد وصل می‌شوند و همه یک لحظه از یک آهنگ را می‌شنوند. بدون سرور؛ همهٔ گوشی‌ها باید روی یک وای‌فای باشند.

**نحوه استفاده:** تنظیمات › «گوش دادن مشترک».
- **میزبان:** «ساخت اتاق» را بزنید. کد ۶ رقمی و آدرس (مثل `192.168.1.5:4000`) نشان داده می‌شود. «کنترل مهمان‌ها» تعیین می‌کند مهمان‌ها بتوانند پخش/توقف/بعدی/قبلی را بزنند یا نه. «بستن اتاق» همه را قطع می‌کند.
- **مهمان:** در بخش «پیوستن» اتاق خودکار پیدا می‌شود؛ رویش بزنید (یا آدرس را دستی بنویسید)، کد را وارد و «اتصال» را بزنید. آهنگ میزبان از گوشی او استریم و پخش می‌شود، پس لازم نیست آهنگ را داشته باشید.
- «جبران تاخیر صدا» (±۵۰۰ ms): برای بلندگوی بلوتوثی که صدا را دیرتر می‌رساند.

**کارکرد:** میزبان یک سرور TCP کوچک دارد (JSON برای وضعیت و فرمان؛ HTTP با Range برای خود فایل صوتی) و با mDNS (`_navaq._tcp`) خودش را معرفی می‌کند. مهمان با پینگ‌های مکرر اختلاف ساعت را تخمین می‌زند و جایگاه میزبان را به «همین لحظه» تبدیل می‌کند؛ اختلاف کم با تغییر ملایم سرعت (±۶٪) و اختلاف زیاد با پرش اصلاح می‌شود. آهنگ بعدی از پیش بارگذاری می‌شود. امنیت: توکن لینک فایل فقط بعد از درست‌بودن کد صادر می‌شود و بعد از ۱۰ کد اشتباه میزبان کد جدید نمی‌پذیرد. ترافیک روی شبکهٔ محلی رمزگذاری نشده (HTTP) است؛ در شبکه‌های عمومی استفاده نکنید. یک سرویس پیش‌زمینه اتاق را در پس‌زمینه زنده نگه می‌دارد.

**محدودیت:** پخش از راه اینترنت (بیرون از یک وای‌فای) نیازمند سرور واسط است و ساخته نشده.

### 9. NAVAQ Downloader

**چیست:** دانلودر رسانه با صف پایدار، مکث و ادامه، تلاش دوباره، کنترل مصرف اینترنت و نمایش سرعت و زمان تقریبی باقی‌مانده.

**منابع پشتیبانی‌شده:** در نسخهٔ فعلی، دریافت رسانه به فایل‌های عمومی Wikimedia Commons محدود است. لینک صفحهٔ فایل یا لینک مستقیم رسانه را وارد کنید؛ لینک‌های دامنه‌های دیگر پذیرفته نمی‌شوند.

**نحوه استفاده:** تنظیمات › «دانلودها».
1. لینک صفحهٔ فایل یا فایل رسانه‌ای عمومی را وارد کنید.
2. در صورت وجود چند رسانه، مورد موردنظر را انتخاب کنید.
3. «فقط صدا» را برای استخراج صدای قابل‌استخراج از فایل ویدیویی فعال کنید.
4. «فقط با وای‌فای» را برای جلوگیری از مصرف اینترنت همراه فعال کنید.
5. در صف دانلود، سرعت، درصد پیشرفت و زمان باقی‌مانده را ببینید؛ هر مورد را مکث، ادامه، تلاش دوباره یا حذف کنید.

**پایداری:** دانلود مستقیم از فایل موقت استفاده می‌کند و در صورت پشتیبانی سرور از HTTP Range، پس از توقف یا بسته‌شدن فرایند از محل قبلی ادامه می‌یابد. پاسخ سرور و مقدار شروع Content-Range قبل از افزودن داده به فایل بررسی می‌شود. دانلودهای HLS عمومی فقط در صورت پشتیبانی قالب و بدون رمزگذاری قابل دریافت‌اند.

**محدودیت و مجوز:** این دانلودر استخراج‌گر عمومی سایت‌ها نیست و محتوای محافظت‌شده، DRM، پخش زنده، جریان رمزگذاری‌شده یا محدودیت‌های دسترسی را دور نمی‌زند. پیش از استفادهٔ مجدد از فایل، مجوز و الزام درج نام سازنده را بررسی کنید. تنظیم شبکهٔ Wi-Fi-only هنگام تغییر شبکه، دانلود را متوقف می‌کند تا کاربر آن را پس از اتصال Wi-Fi ادامه دهد.

**جزئیات فنی:** android/DOWNLOADER.md

### 10. کشف موسیقی (Discover)

**نحوه استفاده:** تنظیمات › «پخش» › «کشف موسیقی». جستجوی فایل‌های صوتی/تصویری عمومی Wikimedia Commons، مرورگر داخلی، و دانلود فایل‌های عمومی همان منبع با DownloadManager اندروید. جزئیات: [`android/DISCOVER.md`](android/DISCOVER.md).

### 11. دستیار صوتی AVA

**نحوه استفاده:** تنظیمات › «پخش» › «دستیار صوتی AVA». بار اول مدل گفتار آفلاین (حدود ۱۱۶ مگابایت) نصب می‌شود؛ بعد از آن تشخیص گفتار کاملاً روی گوشی انجام می‌شود. دستور بدهید: پخش آهنگ، جستجوی هنرمند، باز کردن تنظیمات وای‌فای/بلوتوث/باتری/اعلان، دوربین، پیام‌ها، آلارم، یا آماده‌کردن شماره‌گیر. NAVAQ هرگز خودش تماس نمی‌گیرد؛ شماره را در شماره‌گیر سیستم می‌گذارد و شما خودتان می‌زنید. جزئیات: [`android/AVA.md`](android/AVA.md).

### 12. کنترل‌های سیستم و اعلان‌ها

اعلان‌های هوشمند اختیاری‌اند (از صفحهٔ AVA روشن می‌شوند) و در سه کانال جدا هستند: فعالیت، دانلود، هشدار پخش. اعلان پخش را Media3 مدیریت می‌کند. روی اندروید ۱۳ به بالا مجوز اعلان هنگام شروع درخواست می‌شود.

### 13. پشتیبان و بازنشانی

**نحوه استفاده:** تنظیمات › «پشتیبان و بازنشانی».
- «ذخیرهٔ پشتیبان»: یک فایل JSON در جایی که انتخاب می‌کنید ذخیره می‌شود و شامل تم و تنظیمات، علاقه‌مندی‌ها، پلی‌لیست‌ها، آهنگ‌های ساخته‌شده، آمار، آهنگ‌های پنهان، متن‌ها و تنظیمات صدا است.
- «بازیابی از پشتیبان»: فایل را انتخاب کنید؛ داده‌های فعلی جایگزین می‌شوند و برنامه دوباره بارگذاری می‌شود. فایلی که پشتیبان NAVAQ نباشد رد می‌شود و چیزی تغییر نمی‌کند.
- «بازگشت به پیش‌فرض»: فقط ظاهر برنامه به حالت اولیه برمی‌گردد.

**کارکرد:** محتوای SharedPreferencesهای برنامه با نوع هر مقدار در JSON نوشته و برگردانده می‌شود. خود فایل‌های موسیقی در پشتیبان نیستند.

### 14. منوی آهنگ

**نحوه استفاده:** روی هر آهنگ در فهرست انگشت را نگه دارید: پخش بعدی، افزودن به صف، افزودن به پلی‌لیست، علاقه‌مندی، اشتراک‌گذاری، اطلاعات آهنگ (قالب، نرخ بیت، سال، تعداد پخش) و پنهان‌کردن. آهنگ پنهان از همهٔ فهرست‌ها و جستجو حذف می‌شود و از تنظیمات › «کتابخانه» › «آهنگ‌های پنهان» برمی‌گردد.

**کارکرد:** «پخش بعدی» آهنگ را بلافاصله بعد از آهنگ فعلی در صف Media3 درج می‌کند. اشتراک‌گذاری برای آهنگ‌های حافظهٔ گوشی خود فایل را می‌فرستد و برای آهنگ‌های دانلودی فقط نام آهنگ را.

### 15. متن همگام آهنگ (LRC)

**نحوه استفاده:** در پخش‌کنندهٔ تمام‌صفحه دکمهٔ «متن» را بزنید. متن به یکی از دو راه پیدا می‌شود: فایل `.lrc` هم‌نام آهنگ و کنار آن، یا متنی که خودتان چسبانده‌اید («چسباندن متن از کلیپ‌بورد»؛ LRC همگام یا متن ساده). سطر در حال پخش با رنگ تأکید برجسته می‌شود و صفحه خودکار می‌چرخد؛ روی هر سطر بزنید تا پخش به همان لحظه برود. «حذف این متن» متن چسبانده‌شده را پاک می‌کند.

**کارکرد:** برچسب‌های زمانی `[mm:ss.xx]` (چندتایی در یک سطر) و `[offset:…]` خوانده می‌شوند؛ متن بدون زمان به‌صورت ساده نشان داده می‌شود.

### 16. آمار گوش‌دادن

**نحوه استفاده:** در صفحهٔ اصلی دو تب «اخیراً» و «پرشنوی‌ها» اضافه شده است. تنظیمات › «آمار گوش‌دادن»: مجموع ساعت گوش‌دادن، تعداد پخش‌ها، آهنگ‌های متفاوت، ۸ آهنگ و ۵ هنرمند برتر، و «پاک‌کردن آمار».

**کارکرد:** هر آهنگ پس از ۳۰ ثانیه گوش‌دادن واقعی یک پخش حساب می‌شود (پرش‌ها و ثانیه‌های مکث‌شده حساب نمی‌شوند). آمار فقط روی خود گوشی ذخیره می‌شود.

### 17. ادامه پخش، محو‌شدن تایمر خواب و رد کردن سکوت

- **ادامه:** وقتی برنامه را باز می‌کنید و چیزی پخش نمی‌شود، در تب «آهنگ‌ها» دکمهٔ «ادامه» صف و جایگاه آخرین پخش را برمی‌گرداند.
- **تایمر خواب:** صدا در حدود ۶ ثانیه آرام کم می‌شود و بعد پخش متوقف می‌شود (نه قطع ناگهانی). گزینهٔ «پایان این آهنگ» هم هست.
- **رد کردن سکوت:** تنظیمات › «پخش» › «رد کردن سکوت»؛ بخش‌های ساکت را ExoPlayer رد می‌کند.

---

## ساخت برنامه

```sh
android/scripts/build-rust-android.sh   # کتابخانه‌های Rust (به NDK نیاز دارد)
gradle :android:assembleDebug
```

نیازمندی‌ها: Android SDK ‏(API 36)، JDK 17 و (برای ساخت هسته) Rust + Android NDK. تست هسته: `cargo test --workspace`.

## چه چیزهایی هنوز ساخته نشده

- گوش دادن مشترک از راه اینترنت (نیاز به سرور واسط).
- رندر دوگوشی HRTF کامل و ردیابی سر.
- مدل هوش مصنوعی مولد برای ساخت آهنگ و جداسازی استم‌ها.
- پخش از طریق صف `PlayerCore` در Rust (پخش فعلاً مستقیم با Media3 است).
- نسخهٔ iOS.

</div>

---

# English technical reference

NAVAQ is a music application project with a platform-independent Rust playback/domain core and a native Android host. The Android app includes Media3 playback, AVA voice controls, and the Discover experience described below. 
## Workspace

- `core/` — playback state, command/event contracts, queue management, audio-session policy, snapshot recovery, equalizer/crossfade settings, library, playlists, lyrics (LRC), sleep timer, play history, **and exclusive features** (Shared Listening Rooms, Music DNA matching, creative tools, advanced haptics, personalized spatial audio).
- `android/` — native Android app using AndroidX Media3, MediaSessionService, a JNI bridge to Rust, AVA voice controls, and NAVAQ Discover and Downloader (public audio search, in-app browser, direct audio/video downloads, Wi-Fi/mobile-data policy, and offline download list).
- The iOS AVFoundation adapter is not yet implemented.

## Core boundary

`PlayerCore` owns deterministic playback state and queue decisions. It does not decode audio or access speakers. A native backend is responsible for preparing and rendering audio, managing OS audio sessions, and reporting callbacks such as prepared, position, buffering, and errors.

`AudioBackend` is the adapter contract. Native implementations execute ordered backend actions and return asynchronous callbacks through non-blocking event polling. The runtime connects that contract to the player core.

## Playback semantics

- Resume/Play continue from the current position and only re-prepare when the native item was released (Stop, completion, failure) or never loaded.
- Previous restarts the track after 3 s of playback; at the start of the queue it restarts the first track (or wraps with repeat-all).
- Late `Position` callbacks after Stop/completion and `Ended` callbacks that race with Pause are ignored.
- Track ids must be unique within a queue; track metadata and source identifiers are validated at queue insertion and snapshot restoration. The host should mint distinct ids if the same file appears twice.

## Playback persistence

`PlayerCore::snapshot()` captures the versioned logical playback state and the complete queue order, including shuffle state. `PlayerCore::restore()` validates the snapshot and returns the restored core plus the actions needed to re-prepare the current track.

`PlayerRuntime::restore()` additionally dispatches those actions to a supplied native backend. Restored playback is deliberately paused rather than automatically resumed; the host must re-establish its audio session and decide when to resume. Snapshot data can be serialized with Serde (for example, JSON) and stored by the host application.

## Android platform layer

The Android application module lives in `android/`. It uses Media3 ExoPlayer for native audio decoding/output and MediaSessionService for background playback and system media controls. Android audio focus and noisy-audio handling are delegated to Media3. The Rust core is compiled as a JNI shared library; `android/scripts/build-rust-android.sh` builds the `arm64-v8a` and `x86_64` libraries when the Android NDK is installed.

Build the Android APK after compiling the Rust JNI libraries:

```sh
android/scripts/build-rust-android.sh
gradle :android:assembleDebug
```

The Android playback service publishes normalized title, display title, artist, album, and optional artwork URI through Media3 `MediaSession` metadata. Playback state and supported transport controls are exposed by the session and its automatically maintained MediaStyle notification. This is the public Android interoperability path used by compatible OEM media surfaces; Xiaomi HyperOS Super Island and Samsung One UI Now Bar decide whether and how to present a third-party session based on device model, OS version, settings, and OEM policy. NAVAQ does not use private OEM APIs or claim that an island can be forcibly displayed. Actual HyperOS/One UI presentation and artwork loading still require testing on physical supported devices. The Android service currently bridges player commands, backend preparation/buffering/position/end/error callbacks, and supported Media3 actions. `NavaqHaptics` provides an opt-in, rate-limited Android vibrator adapter for classified bass/snare/hi-hat/vocal/beat events; it is disabled by default and is driven by the audio-processing path (generic onsets, or the Rust `HapticAnalyzer` when advanced haptics is enabled). The platform-independent core now includes stateful PCM equalizer/gain processing and equal-power crossfade mixing primitives in `core/src/dsp.rs`. It also exposes `PcmAudioPipeline` in `core/src/audio_pipeline.rs`, which composes equalizer, ReplayGain/master gain, optional stateful echo, peak limiting, non-finite input sanitization before stateful filters, and PCM meter statistics (including sanitized/limited sample counts) for native decoders. On Android the Media3 service inserts the pipeline into its audio sink through `NavaqBeatAudioProcessor` (equalizer, room reverb, peak ceiling). The launcher is `ui.NavaqHomeActivity` (native glass UI: library, search, player, settings). Discover and AVA are reachable from Settings. Downloads are stored in app-specific Music/NAVAQ storage and merged into the library; the library itself is read from MediaStore.

## NAVAQ Discover (Android)

Discover is a native Android feature reachable from the AVA home screen. It is separate from the Media3 playback service and does not replace or modify the existing playback engine.

### What is included

- **Public audio search:** searches Wikimedia Commons for audio files, displays the title, MIME type and available file size, and supports loading subsequent result pages.
- **In-app browser:** an Android WebView with DuckDuckGo as the default search engine, address/search input, back/forward/reload controls, and HTTP(S)-only top-level navigation.
- **Public audio/video downloads:** searches Wikimedia Commons for public media and downloads supported direct HTTPS media files from its media host only. Supported common formats include MP4, WebM, MKV, MOV, 3GP, AVI, MP3, M4A/AAC, FLAC, OGG, WAV and Opus.
- **Network controls:** choose Wi-Fi only or Wi-Fi plus mobile data for newly queued downloads; roaming is disabled.
- **Download queue and offline list:** stores DownloadManager IDs in app preferences, shows task states and percentage progress, allows active tasks to be cancelled and finished entries to be removed, and opens completed audio/video files with an Android media app.
- **Rust URL validation:** the Android bridge calls `nativeValidateDownloadUrl`, which delegates URL policy checks to `core/src/downloads.rs`. The Android host also checks the initial DNS result for local/private addresses before enqueueing.

### Build and entry point

Open NAVAQ, then choose **Open NAVAQ Discover** from the AVA home screen. To build the Android app locally:

```sh
android/scripts/build-rust-android.sh
gradle :android:assembleDebug
```

### Important boundaries

- Structured audio search currently uses Wikimedia Commons; the Browser tab is general web browsing/search, not a universal multi-site audio index.
- Downloads are restricted to public Wikimedia Commons media files hosted on `upload.wikimedia.org`; media URLs from other hosts are rejected. The Browser tab remains for ordinary web browsing and does not extract media from arbitrary pages.
- Check the license and attribution terms for each Commons file before reuse. DRM, protected streams, authentication, paywalls, age gates and site restrictions are not bypassed.
- Discover uses Android DownloadManager (no in-app pause/resume). The separate **NAVAQ Downloader** (Settings > Downloads, `net/LinkDownloader.kt`) is a custom engine for media files, unencrypted HLS VOD and pages that embed media, with pause/resume; it has no site-specific extractors, DRM or encrypted-stream support.
- The initial hostname is checked before enqueueing, but DownloadManager follows redirects itself. This client-side check is not a complete server-side SSRF boundary.
- Files are saved under app-specific external Music/NAVAQ storage. They are available offline while retained by Android, but may be removed when the app is uninstalled. Search and browsing require internet access.
- The offline list is scoped to downloads started by Discover; it is not a full device-wide MediaStore music scanner.

## Development

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Pure-logic modules

- `Library` — unique tracks, upsert/remove, multi-word Unicode search, sorting, albums/artists.
- `PlaylistStore` — create/rename/delete, add/remove/move tracks, resolve to `Track`s for `SetQueue`.
- `Lyrics::parse_lrc` — synchronized lyrics with multi-timestamp lines and `[offset:]`; `line_at(position_ms)`.
- `EqualizerSettings` + presets — validated, applied through `SetEqualizer`; `SetCrossfade` (0–12000 ms).
- `SleepTimer` — duration or end-of-track, driven by `PlayerRuntime::tick` / `poll_backend`.
- `PlayHistory` — bounded recent plays and play counts; the host calls `record_play`.
- `replaygain` — `linear_gain` / `effective_volume` from ReplayGain tags (track/album mode, preamp, clipping guard).
- `audio_analysis` — streaming PCM energy-onset detector with partial-frame buffering, adaptive noise floor, sensitivity threshold, refractory window and timestamped onset events. It detects generic transients; it does not claim kick/snare/vocal classification.
- `dsp` — stateful multi-band peaking-EQ and gain processing for interleaved f32 PCM, plus equal-power crossfade mixing; native backends own PCM pipeline integration.
- `m3u` — parse and write M3U/M3U8 with `#EXTINF`.
- `guess_from_filename` — track number, artist and title from file names when tags are missing.
- `BookmarkStore` — resume positions for long tracks (ignores the first 5 s and the last 10 s).
- `UserRatings` — favorites and 1-5 ratings, resolvable to tracks for `SetQueue`.
- `text` — `normalize_for_search` unifies Persian/Arabic letter variants, digits, diacritics and ZWNJ; `fuzzy_score` is an fzf-style subsequence matcher. `Library::search` uses the normalization, `Library::search_fuzzy` ranks fuzzy matches, `Library::find_duplicates` groups likely duplicate tracks.
- `cue` — `parse_cue` reads CUE sheets (FLAC+CUE albums) into track start/end times.
- `ScrobbleTracker` — the 30 s / half-track / 4 min listening rule, built into `PlayerCore`: it emits `PlayerEvent::ScrobbleReady(id)` once per qualifying play; seeks and A-B loop jumps are not counted.

### Exclusive / differentiating modules

- **`social`** — Shared Listening Rooms state model with revision-checked commands, participant membership, host-only shared-track/global-mix changes, shared transport updates, latency-aware drift correction (gentle playback-rate adjustment or hard seek), private effects, and participant-contributed room-wide effects (Echo, Reverb, PitchShift, VocalOverlay, Custom). `RoomStore` is in-memory; WebSocket/WebRTC transport, clock-drift correction, presence, moderation and real-time audio mixing still belong to a server/native host.
- **`music_dna`** — Taste profiles built from play history + ratings + optional genre/tempo/energy tags. Similarity scores normalize vectors and validate inputs. `rank_opted_in_matches` excludes profiles that have not explicitly enabled discovery and supports a minimum history threshold. `rank_collaborative_tracks` combines per-participant track-affinity scores into a consensus ranking and penalizes missing coverage; account privacy, consent UI, candidate score generation and collaborative playlist persistence remain host responsibilities.
- **`creative` + `composer`** — Lyric-to-Melody and instrumental/stem-separation job contracts plus a manual multi-track composition model (tempo, meter, key, bars, instruments, notes, velocity, volume and pan). Manual arrangements are validated, can be queued for rendering, and can be exported as a Standard MIDI File. Actual synthesis, AI melody generation and stem separation require a host renderer/model and are not bundled into this Rust crate.
- **`haptic`** — Haptic patterns for BassHit, Snare, HiHat, VocalOnset and Beat, with intensity scaling and trigger gating. A host still needs beat/onset analysis and platform vibrator/controller adapters.
- **`spatial`** — Personalized room dimensions, surface absorption, validated speaker layouts, listener position, HRTF profile and a Sabine-style RT60 estimate. A real HRTF convolution/binaural renderer and device/head tracking are not included.

#### Behaviour layers on top of these modules

- **`ui_settings`** — user-customisable theme/interface schema (`UiSettings`: theme mode, accent, wallpaper, glass blur/opacity, corner radius, font scale, density, cover shape, list/nav style, effects, tabs) with presets, validation and `sanitized()` for older or hand-edited files. The interactive glass UI prototype in `design/ui-simulator/index.html` (open in a browser) uses the same settings.
- **`room_control`** — simultaneous control of a `SharedRoom`. `apply_control(from, ControlCommand, now_ms)` (alongside the revision-checked `apply_command`) accepts Play/Pause/Seek/SetTrack/Enqueue/SkipToNext and room-wide effects from anyone allowed by `ControlMode` (`HostOnly`, `Everyone`, `Delegated` + grants); personal effects (echo, vocal overlay...) need no control right. The host clock decides ordering, every accepted command bumps `revision`, `current_position_ms(now)` gives the room clock, `correct_drift` tells a client to do nothing, nudge speed by ±4 % or seek, and `advance_if_finished` moves through the shared queue.
- **`matching`** — `suggest_friends` (closest taste, with shared artists/genres as the reason, skipping friends), `build_shared_playlist` (average affinity of everyone, duplicates merged, fair share per person, no same artist back to back).
- **`haptic_analyzer`** — `HapticAnalyzer` detects bass/kick, snare-like and hi-hat transients in streaming PCM (three bands, adaptive threshold, refractory time); `schedule_pulses` turns the events into timed pulses using `HapticSettings` and master intensity.
- **`spatial_render`** — `render_spatial(&SpatialSettings)` computes per-speaker distance, azimuth, delay, time-alignment delay, level trim, interaural time difference, first-order wall reflections and the room's RT60 (Sabine) with furniture absorption (`FurnitureItem`: sofa, bed, curtains, carpet, ...). The host's spatializer applies the numbers.
- **`composition`** — manual instrumentals. Write notes per `Part` (sine/square/saw/triangle/pluck/pad and kick/snare/hi-hat), use `DrumPattern::from_rows` (`"x...X..."` step sequencer) and `chord_pitches`, then `render_pcm`, `to_wav_bytes` or `to_midi_bytes`.
- **`melody`** — offline lyric-to-melody: `generate_melody(lyrics, &MelodyOptions)` or `melody_from_request(&LyricToMelodyRequest)` builds melody, pad chords, bass and drums as a `Composition` (styles: pop, sad, rock, lofi, jazz, persian, cinematic; key and tempo hints; Persian lyrics supported; deterministic per lyrics and seed). Render it with `composition` and play the file like any track.

Player modes: `SetConsume` (drop tracks once played), `SetSingle` (stop after the current track; repeat-one still repeats), `SetLoopRegion` (A-B loop, cleared when the track changes), and `Reshuffle { seed }` (new random upcoming order; the current and played tracks stay put). The consume/single semantics follow MPD's playback options; the rest follows common practice in players such as MPD, Auxio and rodio-based apps. These projects were used as design references; no code was copied.

These modules do no I/O. Scanning files, reading tags, networking, AI inference, haptic hardware and spatial rendering are host responsibilities.

## Current implementation status

Implemented: everything in the modules above plus typed track/source models, playback state, command/event contract, queue operations (move, clear, unique ids, next-track lookup for gapless preloading), repeat/shuffle, native backend contract, asynchronous event runtime, audio-focus handling with ducking, failure recovery, versioned snapshots with safe paused restoration, and unit tests.

Implemented in the Android host (not in the core crate): LAN shared rooms with clock-synchronised playback, Music DNA code exchange, user playlists, the NAVAQ Downloader, Rust-DSP-backed equalizer and room reverb, Rust haptic analyzer wiring, song creation (WAV/MIDI), and the native glass UI.

Not built: internet-wide shared rooms (needs a relay server, accounts, moderation); on-device source separation and generative-AI melody models; full binaural/HRTF rendering and head tracking; routing playback through the Rust `PlayerCore` queue (Media3 owns the queue today); and the iOS AVFoundation adapter.


### Social, sensory and creative integration status

The core APIs are intentionally platform-neutral. The following boundaries are
important when planning an end-to-end release:

| Feature | In repository | Still required for an internet-scale product |
| --- | --- | --- |
| Shared Listening Rooms | Core room model and drift correction; **Android LAN rooms** (pairing code, mDNS, HTTP audio, clock-synced playback) | Authenticated relay server, WebSocket/WebRTC transport, reconnect and moderation flows |
| Music DNA Matching | Core profile/similarity/ranking; **Android code exchange** compared on-device | Discovery/privacy controls, backend candidate discovery, block/report, invitations |
| Haptic feedback | Patterns, Rust band analyzer, Android vibrator adapter, settings toggles | Device QA across vibration hardware |
| Personalized spatial audio | Room/speaker models, RT60, per-speaker numbers; simplified room reverb in the Rust PCM chain | HRTF dataset/renderer, head tracking, calibrated measurements |
| Manual instrumental creation | Note/instrument arrangement, WAV render, MIDI export | Editing UI, instrument/SoundFont renderer, share workflow |
| Lyric-to-Melody | Offline rule-based generator (`melody`), Android UI, WAV/MIDI output | A generative model if AI-quality results are wanted |
| Instrumental/stem creation | Stem request/result contracts | A source-separation model |

The `audio_analysis` module now supplies timestamped generic onset events suitable for a host-controlled beat-reactive path; the Android service maps them to `HapticTrigger::Beat`; the optional `HapticAnalyzer` path classifies bass, snare and hi-hat bands. Neither infers vocal onsets. The Android haptic adapter accepts only classified events from a trusted host
component. It does not record microphone audio or perform background capture.
The app must expose a clear user toggle before dispatching events. For shared
rooms, participant-generated vocal/effect streams must be explicitly enabled
and mixed by the room host/server; the Rust room model alone is not a network
audio mixer.
