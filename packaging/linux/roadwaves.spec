Name:           roadwaves
Version:        %{?rw_version}%{!?rw_version:0.1.0}
Release:        1%{?dist}
Summary:        Location-aware ETS2 Radio Garden companion
License:        MIT
BuildArch:      x86_64
%global rw_payload %{_sourcedir}

%description
RoadWaves routes selected Radio Garden MP3 streams to Euro Truck Simulator 2's native Internet Radio player.

%install
install -D -m 0755 %{rw_payload}/roadwaves %{buildroot}%{_bindir}/roadwaves
install -D -m 0755 %{rw_payload}/roadwaves-gui %{buildroot}%{_bindir}/roadwaves-gui
install -D -m 0755 %{rw_payload}/roadwaves_scs.so %{buildroot}%{_libdir}/roadwaves/plugins/roadwaves_scs.so
install -D -m 0644 %{rw_payload}/roadwaves.desktop %{buildroot}%{_datadir}/applications/roadwaves.desktop
install -D -m 0644 %{rw_payload}/roadwaves.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/roadwaves.svg
install -D -m 0644 %{rw_payload}/roadwaves.service %{buildroot}/usr/lib/systemd/user/roadwaves.service
install -D -m 0644 %{rw_payload}/README.md %{buildroot}%{_docdir}/roadwaves/README.md
install -D -m 0644 %{rw_payload}/LICENSE %{buildroot}%{_docdir}/roadwaves/LICENSE
install -D -m 0644 %{rw_payload}/IMPLEMENTATION_REPORT.md %{buildroot}%{_docdir}/roadwaves/IMPLEMENTATION_REPORT.md

%files
%{_bindir}/roadwaves
%{_bindir}/roadwaves-gui
%{_libdir}/roadwaves/plugins/roadwaves_scs.so
%{_datadir}/applications/roadwaves.desktop
%{_datadir}/icons/hicolor/scalable/apps/roadwaves.svg
/usr/lib/systemd/user/roadwaves.service
%doc %{_docdir}/roadwaves/README.md
%license %{_docdir}/roadwaves/LICENSE
%doc %{_docdir}/roadwaves/IMPLEMENTATION_REPORT.md

%changelog
* Thu Oct 15 2026 RoadWaves contributors - 0.1.0-1
- Initial local package definition.
